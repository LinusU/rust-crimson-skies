//! Measured host-binding families and the coverage gate (F38-B, re-keyed by
//! F38-D.1).
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
//! * a family whose measured meaning an engine operation can express is
//!   [`ObservedDisposition::Bound`] with its [`MeasuredMeaning`];
//! * every other family is [`ObservedDisposition::Unimplemented`] with the
//!   reason, and it is **counted, never stubbed**: a binding that returned
//!   success would be a fabricated original behaviour (sheet non-negotiable #2).
//!
//! ## The family key is (host, dispatch value) — F38-D.1
//!
//! The owner's static analysis of the decrypted executable (Rally #159,
//! 2026-10-05) measured that `callback($$X$$, <id>, …)`'s `$$X$$` names one of
//! **five native functions**: `$$A$$` = goscallback (`0x407670`), `$$E$$` =
//! uidata (`0x4093a0`), `$$UB$$` = uicontrol (`0x404960`), `$$JI$$` =
//! gospageinit (`0x403680`), `$$NB$$` = goslocalize (`0x404630`); `mail` is not
//! a native call but GOS script-to-script messaging. A family is therefore
//! keyed by **([`MeasuredHost`], dispatch value)**, never by the value alone:
//! the value 6 is spelled under two different natives, so a value-only key
//! merges two unrelated protocols ([`MeasuredNative`] mirrors the producer's
//! native table, wire code for wire code).
//!
//! ## A family is a protocol, not a fixed signature — F38-D.1
//!
//! These natives take a mode/index argument, so one (host, value) legitimately
//! appears with several arities and per-position argument shapes. Variation is
//! **kept visible** ([`MeasuredCall::arg_shapes`], `None` at a position whose
//! shape varied; [`MeasuredCall::arities`], the observed set) and never
//! resolved by picking one reading. F38-C's 994 `no_family` retail sites were
//! this variation inside a protocol, not a corpus inconsistency.
//!
//! ## What is bound, and what stays refused
//!
//! Three families have a measured meaning (owner note, Rally #159):
//! goslocalize loads string `<id>` from the resource module set by
//! `setresource` into its target ([`MeasuredMeaning::LocalizedStringId`]);
//! gospageinit's first argument is the page record the engine looks up
//! ([`MeasuredMeaning::PageArgument`]); `mail` is delivered to the receiving
//! script's `gui_mailbox` `getmessage()` case
//! ([`MeasuredMeaning::ScriptMailboxMessage`]). Every goscallback/uidata/
//! uicontrol value stays [`ObservedDisposition::Unimplemented`] with the reason
//! [`UnimplementedReason::MeaningNotMeasured`], which names the native and its
//! handler address, and the gate stays closed while one such family exists.
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
    RegistryError, Repeatability,
};

/// Longest measurement-evidence summary a row may carry, in bytes.
///
/// A **safety cap, not a measurement**: the corpus's evidence summaries are
/// short by construction (a note the production reader writes from counts), and
/// no original statement is ever carried here. The bound exists because the row
/// crosses from an untrusted measurement into a `BindingProvenance` and from
/// there into diagnostics, so an unbounded string must not be copied around.
pub const MAX_EVIDENCE_BYTES: usize = 1024;

/// The cap a measured `StringLiteral` argument gets.
///
/// A **safety cap, not a measured length**: the corpus's string arguments have
/// not been measured for length, so this is the bound a binding may declare
/// rather than a fact about the original. It is deliberately far below anything
/// that would look like an original statement, so a bound that later needs to
/// grow has to be argued from a measurement.
pub const MAX_MEASURED_STRING_BYTES: usize = 256;

/// The widest argument protocol one family may claim, in positions.
///
/// Mirrors the producer's
/// `cs_formats::script_raw::ui_host_calls::MAX_HOST_CALL_ARGS`: a family is a
/// protocol whose sites may spell up to the scanner's own argument bound, and
/// the row refuses anything wider rather than truncating it. This is **not**
/// the mission registry's call-argument bound
/// ([`crate::bindings::MAX_CALL_ARGS`]): a UI-script family is never registered
/// in the mission registry, and a protocol width the mission IR cannot declare
/// is not a reason to drop a measured site.
pub const MAX_PROTOCOL_ARGS: usize = 32;

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

/// Which measured dispatch form a call was found in: the **syntactic head**.
///
/// The family key is [`MeasuredHost`] + dispatch value (F38-D.1), not this
/// form: every `callback` site's host is its named native or
/// [`MeasuredHost::Unmeasured`], so keying by form alone would merge five
/// unrelated native protocols. This type survives for the differential trace
/// ([`crate::bindings::differential`]) and for reports of how a site is
/// spelled.
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

/// The five native functions a `callback` site's `$$X$$` target names
/// (F38-D.1; owner static analysis on Rally #159).
///
/// A mirror of `cs_formats::script_raw::ui_host_calls::UiNative`, kept here
/// because `cs_script` may not depend on `cs_formats`. The handler addresses
/// are the owner-supplied decrypted executable's virtual addresses for each
/// native (image sha256 `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`):
/// **static-analysis constants, never `verified_original`**, and a refusal
/// names them so the unmeasured dispatch is traceable back to the image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MeasuredNative {
    /// `$$A$$` — goscallback, handler `0x407670`.
    GosCallback,
    /// `$$E$$` — uidata, handler `0x4093a0`.
    UiData,
    /// `$$UB$$` — uicontrol, handler `0x404960`.
    UiControl,
    /// `$$JI$$` — gospageinit, handler `0x403680`.
    GosPageInit,
    /// `$$NB$$` — goslocalize, handler `0x404630`.
    GosLocalize,
}

impl MeasuredNative {
    /// Every measured native, in wire-code order.
    pub const ALL: [MeasuredNative; 5] = [
        MeasuredNative::GosCallback,
        MeasuredNative::UiData,
        MeasuredNative::UiControl,
        MeasuredNative::GosPageInit,
        MeasuredNative::GosLocalize,
    ];

    /// The stable wire code this native crosses as; 0 is deliberately unused
    /// so a default-zeroed field is refused rather than read as `goscallback`.
    pub const fn code(self) -> u8 {
        match self {
            MeasuredNative::GosCallback => 1,
            MeasuredNative::UiData => 2,
            MeasuredNative::UiControl => 3,
            MeasuredNative::GosPageInit => 4,
            MeasuredNative::GosLocalize => 5,
        }
    }

    /// The native a wire code names; `None` for an unknown code.
    pub const fn from_code(code: u8) -> Option<MeasuredNative> {
        match code {
            1 => Some(MeasuredNative::GosCallback),
            2 => Some(MeasuredNative::UiData),
            3 => Some(MeasuredNative::UiControl),
            4 => Some(MeasuredNative::GosPageInit),
            5 => Some(MeasuredNative::GosLocalize),
            _ => None,
        }
    }

    /// The DebugInfo name of this native, as the owner's table records it;
    /// identical to the producer's vocabulary.
    pub const fn label(self) -> &'static str {
        match self {
            MeasuredNative::GosCallback => "goscallback",
            MeasuredNative::UiData => "uidata",
            MeasuredNative::UiControl => "uicontrol",
            MeasuredNative::GosPageInit => "gospageinit",
            MeasuredNative::GosLocalize => "goslocalize",
        }
    }

    /// The `$$X$$` token the corpus spells as this native's dispatch target.
    pub const fn token(self) -> &'static str {
        match self {
            MeasuredNative::GosCallback => "$$A$$",
            MeasuredNative::UiData => "$$E$$",
            MeasuredNative::UiControl => "$$UB$$",
            MeasuredNative::GosPageInit => "$$JI$$",
            MeasuredNative::GosLocalize => "$$NB$$",
        }
    }

    /// The handler address of this native in the owner-supplied decrypted
    /// executable; see [`MeasuredNative`] for what it establishes.
    pub const fn handler_address(self) -> u32 {
        match self {
            MeasuredNative::GosCallback => 0x407670,
            MeasuredNative::UiData => 0x4093a0,
            MeasuredNative::UiControl => 0x404960,
            MeasuredNative::GosPageInit => 0x403680,
            MeasuredNative::GosLocalize => 0x404630,
        }
    }
}

/// The host half of a family key (F38-D.1): which measured dispatch target a
/// call names.
///
/// A mirror of `cs_formats::script_raw::ui_host_calls::HostKind`. The key of a
/// family is (**host**, dispatch value) — never the value alone: the value 6
/// is spelled under two different natives in the retail corpus, so a value-only
/// key merges two unrelated protocols.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MeasuredHost {
    /// A `callback` site naming one of the five measured natives.
    Native(MeasuredNative),
    /// A `mail` site: GOS script-to-script messaging, not a native call. Its
    /// dispatch value is the message id.
    Mail,
    /// A `callback` site whose `$$X$$` target is not one of the five measured
    /// natives: counted and keyed, never folded into a native family.
    Unmeasured,
}

impl MeasuredHost {
    /// Stable lowercase label for reports; identical to the producer's
    /// vocabulary.
    pub const fn label(self) -> &'static str {
        match self {
            MeasuredHost::Native(native) => native.label(),
            MeasuredHost::Mail => "mail",
            MeasuredHost::Unmeasured => "unmeasured",
        }
    }

    /// The wire code this host crosses as; native codes pass through, and
    /// `mail` / `unmeasured` take the two codes above the native range.
    pub const fn code(self) -> u8 {
        match self {
            MeasuredHost::Native(native) => native.code(),
            MeasuredHost::Mail => 6,
            MeasuredHost::Unmeasured => 7,
        }
    }

    /// The host a wire code names; `None` for an unknown code.
    pub const fn from_code(code: u8) -> Option<MeasuredHost> {
        match code {
            1 => Some(MeasuredHost::Native(MeasuredNative::GosCallback)),
            2 => Some(MeasuredHost::Native(MeasuredNative::UiData)),
            3 => Some(MeasuredHost::Native(MeasuredNative::UiControl)),
            4 => Some(MeasuredHost::Native(MeasuredNative::GosPageInit)),
            5 => Some(MeasuredHost::Native(MeasuredNative::GosLocalize)),
            6 => Some(MeasuredHost::Mail),
            7 => Some(MeasuredHost::Unmeasured),
            _ => None,
        }
    }

    /// The dispatch form this host is spelled in: every host but
    /// [`MeasuredHost::Mail`] is a `callback` site.
    pub const fn form(self) -> MeasuredForm {
        match self {
            MeasuredHost::Mail => MeasuredForm::Mail,
            MeasuredHost::Native(_) | MeasuredHost::Unmeasured => MeasuredForm::Callback,
        }
    }

    /// Whether this host is one of the five measured natives.
    pub const fn is_native(self) -> bool {
        matches!(self, MeasuredHost::Native(_))
    }
}

/// One measured (host, dispatch value) protocol as it crosses into the
/// engine: the raw measurement facts and nothing else.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredCallRow {
    /// The dispatch host: the named native, `mail`, or an unmeasured target.
    pub host: MeasuredHost,
    /// The integer the sites spell as their dispatch expression; `None` when
    /// the dispatch expression is not an integer literal (the host's
    /// computed-dispatch protocol).
    pub native_id: Option<i64>,
    /// Sites measured across the corpus.
    pub sites: u32,
    /// Programs that contributed a site.
    pub scripts: u32,
    /// Every argument count observed for this value, ascending.
    pub arities: Vec<u32>,
    /// Per argument position, the shape every observed site agreed on, or `None`
    /// when the sites varied there — the natives take a mode/index argument, so
    /// a varying position is protocol variation, recorded not resolved.
    pub arg_shapes: Vec<Option<MeasuredShape>>,
    /// A summary of how the row was measured and where; never original text.
    pub evidence: String,
}

/// Why a measured row cannot become a family.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowError {
    /// The dispatch value is outside the engine's id range.
    IdOutOfRange {
        /// The measured value.
        native_id: i64,
    },
    /// The row claims more arguments than a protocol may carry.
    TooManyArguments {
        /// The measured value; `None` for a computed dispatch.
        native_id: Option<i64>,
        /// Argument positions the row claims.
        args: usize,
        /// The bound in force.
        limit: usize,
    },
    /// The row claims sites but no observed argument count, so no protocol
    /// width can be stated for it.
    NoArity {
        /// The measured value; `None` for a computed dispatch.
        native_id: Option<i64>,
    },
    /// The row's shape table does not cover its own widest observed arity, so
    /// a site at that arity would be judged only up to the table's width and
    /// its remaining positions would go unchecked.
    ProtocolWidthMismatch {
        /// The measured value; `None` for a computed dispatch.
        native_id: Option<i64>,
        /// Argument positions the shape table claims.
        args: usize,
        /// The widest argument count the row observed.
        widest_arity: u32,
    },
    /// The row carries no evidence, or a summary over
    /// [`MAX_EVIDENCE_BYTES`].
    NoEvidence {
        /// The measured value; `None` for a computed dispatch.
        native_id: Option<i64>,
    },
}

impl RowError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::IdOutOfRange { .. } => "id_out_of_range",
            Self::TooManyArguments { .. } => "too_many_arguments",
            Self::NoArity { .. } => "no_arity",
            Self::ProtocolWidthMismatch { .. } => "protocol_width_mismatch",
            Self::NoEvidence { .. } => "no_evidence",
        }
    }
}

/// The dispatch value a row error is about, in diagnostic words.
fn describe_id(native_id: Option<i64>) -> String {
    match native_id {
        Some(id) => format!("dispatch value {id}"),
        None => "a computed dispatch expression".to_owned(),
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
                "{} claims {args} arguments, at most {limit}",
                describe_id(*native_id)
            ),
            Self::NoArity { native_id } => {
                write!(f, "{}: no observed argument count", describe_id(*native_id))
            }
            Self::ProtocolWidthMismatch {
                native_id,
                args,
                widest_arity,
            } => write!(
                f,
                "{}: the shape table covers {args} position(s), its widest \\
                 observed arity is {widest_arity}",
                describe_id(*native_id)
            ),
            Self::NoEvidence { native_id } => {
                write!(f, "{}: no measurement evidence", describe_id(*native_id))
            }
        }
    }
}

impl std::error::Error for RowError {}

/// One measured (host, dispatch value) protocol, validated.
///
/// A **protocol**, not a fixed signature: [`MeasuredCall::arities`] is the set
/// of argument counts the corpus observed for this key and a `None` in
/// [`MeasuredCall::arg_shapes`] marks a position whose shape varied with the
/// mode/index argument. The variation is kept, never resolved to one reading.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredCall {
    /// The dispatch host: the named native, `mail`, or an unmeasured target.
    pub host: MeasuredHost,
    /// The dispatch value; `None` for a computed dispatch expression.
    pub native_id: Option<i64>,
    /// Sites measured across the corpus.
    pub sites: u32,
    /// Programs that contributed a site.
    pub scripts: u32,
    /// Every argument count observed, ascending and non-empty.
    pub arities: Vec<u32>,
    /// The agreed shape per argument position; `None` where the sites varied.
    pub arg_shapes: Vec<Option<MeasuredShape>>,
    /// A summary of how the call was measured and where.
    pub evidence: String,
}

impl MeasuredCall {
    /// The registry spelling this call is registered under.
    ///
    /// A **derived** spelling, not an original symbol: the corpus contains the
    /// host and an integer, and no name. It is stable, unambiguous and built
    /// only from measured facts — and it carries the **host**, so two natives
    /// sharing a dispatch value can never collide (`goscallback#6` vs
    /// `uicontrol#6`). A computed dispatch is spelled `#computed`.
    pub fn spelling(&self) -> String {
        let id = match self.native_id {
            Some(id) => id.to_string(),
            None => "computed".to_owned(),
        };
        format!("{}#{id}", self.host.label())
    }

    /// Validates one measured row into a protocol.
    ///
    /// Arity and argument-shape **variation is accepted**: these natives take
    /// a mode/index argument, so one key legitimately appears with several
    /// arities and shapes (owner note, Rally #159). What is refused is a row
    /// that cannot state a protocol at all: a value out of range, more
    /// positions than [`MAX_PROTOCOL_ARGS`], no observed arity, a shape table
    /// that does not cover the widest observed arity, or no evidence.
    ///
    /// # Errors
    ///
    /// [`RowError`] for a dispatch value out of range, an argument width over
    /// [`MAX_PROTOCOL_ARGS`], a row with no observed arity, a shape table
    /// shorter than the widest observed arity, or with an empty or over-long
    /// evidence summary.
    pub fn from_row(row: &MeasuredCallRow) -> Result<Self, RowError> {
        if let Some(native_id) = row.native_id
            && (native_id < 0 || native_id > i64::from(i32::MAX))
        {
            return Err(RowError::IdOutOfRange { native_id });
        }
        if row.arg_shapes.len() > MAX_PROTOCOL_ARGS {
            return Err(RowError::TooManyArguments {
                native_id: row.native_id,
                args: row.arg_shapes.len(),
                limit: MAX_PROTOCOL_ARGS,
            });
        }
        if row.arities.is_empty() {
            return Err(RowError::NoArity {
                native_id: row.native_id,
            });
        }
        // The shape table must cover the protocol's own widest arity: `judge`
        // checks a site's positions against it, and a table shorter than an
        // observed arity would leave that site's trailing positions unchecked.
        // The producer always grows the table to the maximum width its sites
        // spell, so `arg_shapes.len() == widest arity` for every corpus row;
        // a row that disagrees cannot state its protocol.
        let widest_arity = row.arities.iter().copied().max().unwrap_or(0);
        if row.arg_shapes.len() != widest_arity as usize {
            return Err(RowError::ProtocolWidthMismatch {
                native_id: row.native_id,
                args: row.arg_shapes.len(),
                widest_arity,
            });
        }
        if row.evidence.trim().is_empty() || row.evidence.len() > MAX_EVIDENCE_BYTES {
            return Err(RowError::NoEvidence {
                native_id: row.native_id,
            });
        }
        Ok(Self {
            host: row.host,
            native_id: row.native_id,
            sites: row.sites,
            scripts: row.scripts,
            arities: row.arities.clone(),
            arg_shapes: row.arg_shapes.clone(),
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
    ///   [`MAX_MEASURED_STRING_BYTES`], a safety bound rather than a measured
    ///   string length;
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
                max_bytes: MAX_MEASURED_STRING_BYTES,
            }),
            _ => None,
        }
    }

    /// The domains this call's measured shapes imply, or the first position
    /// that has none.
    ///
    /// A position that **varied** (`None`) has no single domain to declare,
    /// and neither does a shape the engine has no value for; both refuse
    /// rather than picking a reading.
    pub fn arg_domains(&self) -> Result<Vec<ArgDomain>, usize> {
        let mut domains = Vec::with_capacity(self.arg_shapes.len());
        for (position, shape) in self.arg_shapes.iter().enumerate() {
            match shape.and_then(Self::arg_domain) {
                Some(domain) => domains.push(domain),
                None => return Err(position),
            }
        }
        Ok(domains)
    }
}

/// A measured meaning: what the owner's static analysis established a family
/// does (owner note, Rally #159).
///
/// Each variant names the measurement it rests on; nothing here is
/// `verified_original`, and nothing here is executed by the mission IR — a
/// measured UI-script host call is executed by the UI host layer, and its
/// family is bound in this table so the coverage audit can tell "measured" from
/// "unmeasured", not so a mission program can call it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasuredMeaning {
    /// goslocalize: loads string `<string id>` from the resource module set by
    /// `setresource` (langui.dll) and writes the text into the target argument
    /// (`0x4050c0` → `0x4050e0`, text written by `0x403350`).
    LocalizedStringId,
    /// gospageinit: the first argument is the page record the engine looks up
    /// (`0x404940`); there is no integer id.
    PageArgument,
    /// `mail`: GOS script-to-script messaging, delivered to the recipient
    /// script's `gui_mailbox` `getmessage()` switch — the message id selects
    /// the receiving script's mailbox case.
    ScriptMailboxMessage,
}

impl MeasuredMeaning {
    /// Stable lowercase label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::LocalizedStringId => "localized_string_id",
            Self::PageArgument => "page_argument",
            Self::ScriptMailboxMessage => "script_mailbox_message",
        }
    }
}

impl fmt::Display for MeasuredMeaning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Why a measured family is not implemented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnimplementedReason {
    /// No original observation states what this (native, dispatch value) does.
    /// The native and its handler address are named so the open question stays
    /// traceable to the image it must be measured from.
    MeaningNotMeasured {
        /// The unmeasured native.
        native: MeasuredNative,
        /// What is missing.
        detail: &'static str,
    },
    /// The dispatch target token is not one of the five measured natives, so
    /// not even the native is known.
    HostNotMeasured {
        /// What is missing.
        detail: &'static str,
    },
}

impl UnimplementedReason {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::MeaningNotMeasured { .. } => "meaning_not_measured",
            Self::HostNotMeasured { .. } => "host_not_measured",
        }
    }
}

impl fmt::Display for UnimplementedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MeaningNotMeasured { native, detail } => write!(
                f,
                "{} (handler 0x{:06x}): {detail}",
                native.label(),
                native.handler_address()
            ),
            Self::HostNotMeasured { detail } => write!(f, "{detail}"),
        }
    }
}

/// What became of one measured family.
#[derive(Clone, Debug, PartialEq)]
pub enum ObservedDisposition {
    /// The family's meaning is measured ([`MeasuredMeaning`]) and the family
    /// is bound under [`MeasuredCall::spelling`].
    ///
    /// It is **not** registered in the mission [`HostBindingRegistry`]: these
    /// are UI-script host calls, the mission IR has no operation for them, and
    /// a mission program referencing one must still fail validation before
    /// flight.
    Bound {
        /// The family's derived spelling.
        spelling: String,
        /// The measured meaning.
        meaning: MeasuredMeaning,
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

/// One measured family: the calls of one dispatch host and one dispatch value.
#[derive(Clone, Debug, PartialEq)]
pub struct ObservedFamily {
    /// The measured call (a protocol).
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
    ///
    /// A corpus that measured **nothing** is not complete either: an empty
    /// family set has nothing unimplemented only because nothing was looked at,
    /// and a gate that answered "complete" for it would fail open on the one
    /// input most likely to be wrong — a measurement that never reached a
    /// program.
    pub fn complete(&self) -> bool {
        self.families > 0 && self.unimplemented_families == 0
    }

    /// The gate itself: `true` only when the corpus is non-empty and every
    /// measured family is implemented.
    pub fn campaign_ready(&self) -> bool {
        self.complete()
    }
}

/// Why a measured corpus was refused as a whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoverageError {
    /// Two rows claim the same dispatch host and value.
    DuplicateCall {
        /// The dispatch host.
        host: MeasuredHost,
        /// The measured value; `None` for a computed dispatch.
        native_id: Option<i64>,
    },
    /// A row is not a valid measured call.
    BadRow(RowError),
    /// A family the table was asked to bind cannot be registered: the derived
    /// spelling is refused or collides.
    ///
    /// **Reserved for the registry path.** The measured UI-script families are
    /// bound with a [`MeasuredMeaning`] and deliberately never registered in
    /// the mission registry, so this variant is not constructed by them; it is
    /// the refusal a rule added to [`register_measured`] must return, and it is
    /// listed here so that such a rule cannot report a family as bound when the
    /// registry refused it.
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
            Self::DuplicateCall { host, native_id } => {
                write!(
                    f,
                    "{} of `{}` measured twice",
                    describe_id(*native_id),
                    host.label()
                )
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
/// the (always mission-empty) registry.
///
/// A family is bound only by a **declared** rule that maps its measured host to
/// a [`MeasuredMeaning`] the owner's static analysis established (F38-D.1:
/// goslocalize, gospageinit and `mail`; see [`classify`]). Every other family
/// is refused with its reason and counted, so the coverage denominator stays
/// the corpus's and the gate stays closed while one family is unmeasured. The
/// registry this table hands out ships nothing: a measured UI-script host call
/// is not a mission operation, and a mission program referencing one must fail
/// validation before flight (AC01).
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
    /// The dedup key is (**host**, dispatch value): the same dispatch value
    /// under two different natives is two families, not a duplicate (the value
    /// 6 is spelled under two natives in the retail corpus). A family whose
    /// host has a measured meaning is bound; every other family is refused and
    /// counted.
    ///
    /// # Errors
    ///
    /// [`CoverageError`] for a duplicate (host, value) pair or a row that is
    /// not a valid measured call. A refused corpus yields no table at all, so a
    /// partial corpus is never mistaken for a whole one.
    pub fn measure(rows: &[MeasuredCallRow]) -> Result<Self, CoverageError> {
        let mut seen = BTreeMap::<(MeasuredHost, Option<i64>), ()>::new();
        let mut families = Vec::with_capacity(rows.len());
        for row in rows {
            if seen.insert((row.host, row.native_id), ()).is_some() {
                return Err(CoverageError::DuplicateCall {
                    host: row.host,
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

    /// The family for one dispatch host and value.
    pub fn family(&self, host: MeasuredHost, native_id: Option<i64>) -> Option<&ObservedFamily> {
        self.families
            .iter()
            .find(|f| f.call.host == host && f.call.native_id == native_id)
    }

    /// The registry holding the bound families: empty for the measured UI
    /// corpus, because a measured host call is not a mission operation.
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

    /// The families of one dispatch host, in row order.
    pub fn families_of(&self, host: MeasuredHost) -> impl Iterator<Item = &ObservedFamily> {
        self.families.iter().filter(move |f| f.call.host == host)
    }
}

/// Decides one measured protocol's disposition.
///
/// This is the only place a measured family becomes a binding, and it binds
/// exactly the three hosts whose meaning the owner's static analysis measured
/// (Rally #159): goslocalize ([`MeasuredMeaning::LocalizedStringId`]),
/// gospageinit ([`MeasuredMeaning::PageArgument`]) and `mail`
/// ([`MeasuredMeaning::ScriptMailboxMessage`]). Every goscallback/uidata/
/// uicontrol value stays [`UnimplementedReason::MeaningNotMeasured`] — its
/// per-value meanings are open on #159 and must be measured from the image,
/// never inferred from a convenient engine operation — and a target token
/// outside the native table stays [`UnimplementedReason::HostNotMeasured`].
fn classify(call: &MeasuredCall) -> ObservedDisposition {
    let meaning = match call.host {
        MeasuredHost::Native(MeasuredNative::GosLocalize) => MeasuredMeaning::LocalizedStringId,
        MeasuredHost::Native(MeasuredNative::GosPageInit) => MeasuredMeaning::PageArgument,
        MeasuredHost::Mail => MeasuredMeaning::ScriptMailboxMessage,
        MeasuredHost::Native(native) => {
            return ObservedDisposition::Unimplemented {
                reason: UnimplementedReason::MeaningNotMeasured {
                    native,
                    detail: "no original observation states what this dispatch value does",
                },
                evidence: call.evidence.clone(),
            };
        }
        MeasuredHost::Unmeasured => {
            return ObservedDisposition::Unimplemented {
                reason: UnimplementedReason::HostNotMeasured {
                    detail: "the dispatch target token is not one of the five measured natives",
                },
                evidence: call.evidence.clone(),
            };
        }
    };
    ObservedDisposition::Bound {
        spelling: call.spelling(),
        meaning,
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
        signatures: vec![args],
        lowering,
        repeatability,
        provenance: BindingProvenance::Observed {
            evidence: call.evidence.clone(),
        },
    };
    registry.register(spec.clone())?;
    Ok(spec)
}
