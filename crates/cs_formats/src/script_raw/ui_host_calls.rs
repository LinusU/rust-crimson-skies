//! The measured native host-call sites of the shipped UI script programs
//! (F38-B, first bounded batch).
//!
//! `specs/F38-original-program-adapters-and-native-behavior-bindings.md`, stage
//! `### F38-B`; shared contract `docs/contracts/SCRIPT-MISSION.md`.
//!
//! The mission programs F13 locates inside `zrdr.zbd` are **not decoded**: the
//! mission language is unmeasured (F13-D has not run), so nothing here reads
//! them and no claim is made about their instructions. The installation does
//! ship one *other* program family that is readable without any assumption:
//! the `ASSETS/SCRIPTS/*.SCRIPT` members of `crimson.rof`, which are ASCII
//! text. This module measures the two native dispatch forms that corpus
//! actually contains and nothing else:
//!
//! * `callback($$<handler>$$, <call>, <arg>…)` — a dispatch to a named handler,
//!   optionally with arguments;
//! * `mail(<message>, <recipient>)` — a message to a recipient object.
//!
//! What is measured is the **shape**, never the meaning: the dispatch form, the
//! enclosing block label, whether the dispatch expression is an integer
//! literal (and then its value), the *class* of each argument expression and
//! the byte span of the statement. No original text is retained, no expression
//! is evaluated and no id is given a meaning: `Some(10558)` means "this site
//! spells the integer 10558", not "this site calls behaviour 10558". The
//! meanings live in the packed executable and are **unknown** — see the
//! recorded unknowns in
//! `docs/findings/2026-10-03-f38-b-observed-host-call-corpus.md`.
//!
//! Everything is bounded and fail-closed: [`UiScriptLimits`] caps the script
//! size, the number of sites, the argument count and every expression's byte
//! length, an unterminated call or an unbalanced block list is refused, and the
//! measurement never panics on untrusted bytes.
//!
//! The scanner is a *reader of text*, not a decoder of a language: it recognises
//! the two measured forms and counts every other call-shaped head
//! ([`UiProgramScan::other_call_heads`]) so the batch's boundary is a measured
//! number rather than an assertion.

use std::collections::BTreeMap;
use std::fmt;

use crate::script_raw::evidence::{
    ByteSpan, Confidence, EvidenceError, EvidenceLocator, ResearchMethod, ScriptEvidence,
};

/// Largest UI script program this scanner accepts, in bytes.
pub const MAX_UI_SCRIPT_BYTES: usize = 1 << 20;
/// Most host-call sites one program may hold.
pub const MAX_HOST_CALL_SITES: usize = 16_384;
/// Most argument expressions one site may hold (the call expression excluded).
pub const MAX_HOST_CALL_ARGS: usize = 32;
/// Longest enclosing block label retained per site.
pub const MAX_BLOCK_LABEL_BYTES: usize = 48;
/// Longest argument expression this scanner will walk, in bytes.
pub const MAX_ARG_EXPR_BYTES: usize = 512;

/// The measured native dispatch form a site was found in.
///
/// A design-free measurement: the two spellings are byte patterns in the
/// corpus, not a claim that the original has exactly two native call kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DispatchForm {
    /// `callback($$handler$$, <call>, <arg>…)`.
    Callback,
    /// `mail(<message>, <recipient>)`.
    Mail,
}

impl DispatchForm {
    /// Stable lowercase label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Callback => "callback",
            Self::Mail => "mail",
        }
    }
}

/// The measured class of one expression in a host call.
///
/// The variants are shape classes only: `IntegerLiteral` records that the
/// expression spells an integer, never what the integer *means*. `Unevaluated`
/// is the honest answer for every form this slice does not evaluate (a
/// parenthesised expression, an arithmetic combination, an indexed member
/// path), and it deliberately carries no text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArgShape {
    /// A decimal integer literal, optionally signed.
    IntegerLiteral,
    /// A floating-point literal.
    FloatLiteral,
    /// A `"…"` string literal.
    StringLiteral,
    /// `$$name$$`, a named reference.
    NameRef,
    /// `@scope@class`, a widget-class reference.
    WidgetClassRef,
    /// A dotted path such as `a.b.c`.
    MemberRef,
    /// An expression containing an index such as `a[i]` or `a[i].b`.
    IndexedRef,
    /// Any other expression: measured as a byte length, never retained.
    Unevaluated,
}

impl ArgShape {
    /// Every shape, in the order of [`ArgShape::ALL`].
    pub const ALL: [ArgShape; 8] = [
        ArgShape::IntegerLiteral,
        ArgShape::FloatLiteral,
        ArgShape::StringLiteral,
        ArgShape::NameRef,
        ArgShape::WidgetClassRef,
        ArgShape::MemberRef,
        ArgShape::IndexedRef,
        ArgShape::Unevaluated,
    ];

    /// Stable numeric code, so a consumer in another crate can carry a measured
    /// shape without restating this enum.
    pub const fn code(self) -> u8 {
        match self {
            ArgShape::IntegerLiteral => 0,
            ArgShape::FloatLiteral => 1,
            ArgShape::StringLiteral => 2,
            ArgShape::NameRef => 3,
            ArgShape::WidgetClassRef => 4,
            ArgShape::MemberRef => 5,
            ArgShape::IndexedRef => 6,
            ArgShape::Unevaluated => 7,
        }
    }

    /// The shape a [`ArgShape::code`] names; `None` for a code this build does
    /// not know, so a stale consumer cannot silently read a wrong shape.
    pub const fn from_code(code: u8) -> Option<ArgShape> {
        match code {
            0 => Some(ArgShape::IntegerLiteral),
            1 => Some(ArgShape::FloatLiteral),
            2 => Some(ArgShape::StringLiteral),
            3 => Some(ArgShape::NameRef),
            4 => Some(ArgShape::WidgetClassRef),
            5 => Some(ArgShape::MemberRef),
            6 => Some(ArgShape::IndexedRef),
            7 => Some(ArgShape::Unevaluated),
            _ => None,
        }
    }

    /// Stable lowercase label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            ArgShape::IntegerLiteral => "integer_literal",
            ArgShape::FloatLiteral => "float_literal",
            ArgShape::StringLiteral => "string_literal",
            ArgShape::NameRef => "name_ref",
            ArgShape::WidgetClassRef => "widget_class_ref",
            ArgShape::MemberRef => "member_ref",
            ArgShape::IndexedRef => "indexed_ref",
            ArgShape::Unevaluated => "unevaluated",
        }
    }
}

/// One measured native host-call site.
#[derive(Clone, Debug, PartialEq)]
pub struct HostCallSite {
    /// Which measured dispatch form this site was found in.
    pub form: DispatchForm,
    /// Label of the enclosing depth-1 block (`main`, `gui_create`, a widget
    /// class name, …). Empty when the site is outside every block. This is a
    /// block label, **not** a claim that the label names a handler: the corpus
    /// puts widget class names at depth 1 as well.
    pub block: String,
    /// The dispatch expression's value when it is an integer literal. `None`
    /// when it is any other expression: the site is still measured, but no id
    /// is claimed for it.
    pub native_id: Option<i64>,
    /// The `callback` form's first expression: the `$$…$$` reference naming the
    /// dispatch target. `None` for a `mail` site, which has no such argument.
    ///
    /// Only its **class** is retained. The referenced name is original text and
    /// this module keeps none, so a report can say "the site targets a named
    /// reference" without naming it.
    pub target: Option<ArgShape>,
    /// The class of each argument expression, in argument order, **excluding**
    /// the dispatch expression and the `callback` target reference. Empty for a
    /// `callback` with no argument and for a `mail` site without a recipient.
    pub args: Vec<ArgShape>,
    /// The byte range of the whole call statement in the member.
    pub span: ByteSpan,
}

/// One call-shaped head that is **not** one of the two measured dispatch forms.
///
/// Counted so the batch's boundary is a measurement: `script_run`,
/// `initialize`, `getmessage` and the rest of the corpus's call-shaped heads
/// are listed here with their counts rather than silently ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OtherCallHead {
    pub head: String,
    pub sites: u32,
}

/// The bounds one scan runs under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiScriptLimits {
    /// [`MAX_UI_SCRIPT_BYTES`].
    pub max_script_bytes: usize,
    /// [`MAX_HOST_CALL_SITES`].
    pub max_sites: usize,
    /// [`MAX_HOST_CALL_ARGS`].
    pub max_args: usize,
    /// [`MAX_ARG_EXPR_BYTES`].
    pub max_expr_bytes: usize,
}

impl Default for UiScriptLimits {
    fn default() -> Self {
        Self {
            max_script_bytes: MAX_UI_SCRIPT_BYTES,
            max_sites: MAX_HOST_CALL_SITES,
            max_args: MAX_HOST_CALL_ARGS,
            max_expr_bytes: MAX_ARG_EXPR_BYTES,
        }
    }
}

/// Why a program was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiScriptError {
    /// The program is larger than [`UiScriptLimits::max_script_bytes`].
    ScriptTooLarge {
        /// Bytes in the program.
        len: usize,
        /// The bound in force.
        limit: usize,
    },
    /// More host-call sites than [`UiScriptLimits::max_sites`].
    TooManySites {
        /// The bound in force.
        limit: usize,
    },
    /// A site carries more arguments than [`UiScriptLimits::max_args`].
    TooManyArguments {
        /// The site offset.
        at: u64,
        /// The bound in force.
        limit: usize,
    },
    /// An argument expression is longer than
    /// [`UiScriptLimits::max_expr_bytes`], so the scanner refuses rather than
    /// walking an unbounded expression.
    ExpressionTooLong {
        /// The argument's byte offset.
        at: u64,
        /// The bound in force.
        limit: usize,
    },
    /// A call's opening parenthesis is never closed inside the program.
    UnterminatedCall {
        /// The call's byte offset.
        at: u64,
    },
    /// The program's braces do not balance, so no site can be attributed to a
    /// block and the scan is refused instead of guessing.
    UnbalancedBlocks {
        /// Blocks opened but never closed.
        opened: usize,
    },
    /// The corpus summary note for a dispatch value could not be recorded as
    /// evidence. A record without its provenance is refused rather than
    /// reported as a bare number.
    EvidenceRefused(EvidenceError),
}

impl UiScriptError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ScriptTooLarge { .. } => "script_too_large",
            Self::TooManySites { .. } => "too_many_sites",
            Self::TooManyArguments { .. } => "too_many_arguments",
            Self::ExpressionTooLong { .. } => "expression_too_long",
            Self::UnterminatedCall { .. } => "unterminated_call",
            Self::UnbalancedBlocks { .. } => "unbalanced_blocks",
            Self::EvidenceRefused(_) => "evidence_refused",
        }
    }
}

impl From<EvidenceError> for UiScriptError {
    fn from(error: EvidenceError) -> Self {
        Self::EvidenceRefused(error)
    }
}

impl fmt::Display for UiScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScriptTooLarge { len, limit } => {
                write!(f, "UI script of {len} bytes exceeds {limit}")
            }
            Self::TooManySites { limit } => write!(f, "more than {limit} host-call sites"),
            Self::TooManyArguments { at, limit } => {
                write!(f, "0x{at:x}: more than {limit} call arguments")
            }
            Self::ExpressionTooLong { at, limit } => {
                write!(f, "0x{at:x}: argument expression exceeds {limit} bytes")
            }
            Self::UnterminatedCall { at } => write!(f, "0x{at:x}: unterminated call"),
            Self::UnbalancedBlocks { opened } => {
                write!(f, "{opened} block(s) opened and never closed")
            }
            Self::EvidenceRefused(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for UiScriptError {}

/// What one scan of one UI script program found.
#[derive(Clone, Debug, PartialEq)]
pub struct UiProgramScan {
    /// Container spelling the program came from, used as the evidence locator.
    pub spelling: String,
    /// Every measured host-call site, in source order.
    pub sites: Vec<HostCallSite>,
    /// Call-shaped heads outside the two measured dispatch forms, in spelling
    /// order with their counts.
    pub other_call_heads: Vec<OtherCallHead>,
}

impl UiProgramScan {
    /// Measured sites whose dispatch expression is an integer literal.
    pub fn sites_with_native_id(&self) -> impl Iterator<Item = &HostCallSite> {
        self.sites.iter().filter(|s| s.native_id.is_some())
    }

    /// Measured sites whose dispatch expression is **not** an integer literal:
    /// the batch cannot name a native id for them, and they are never counted
    /// as one.
    pub fn sites_without_native_id(&self) -> impl Iterator<Item = &HostCallSite> {
        self.sites.iter().filter(|s| s.native_id.is_none())
    }
}

/// Scans one measured UI script program for the two native dispatch forms.
///
/// The scan is bounded and fail-closed; see [`UiScriptLimits`]. Byte ranges are
/// relative to the start of `bytes`.
///
/// # Errors
///
/// [`UiScriptError`] for a program over a bound, an unterminated call, an
/// argument expression over its bound or unbalanced blocks.
pub fn scan_ui_program(
    spelling: impl Into<String>,
    bytes: &[u8],
    limits: UiScriptLimits,
) -> Result<UiProgramScan, UiScriptError> {
    if bytes.len() > limits.max_script_bytes {
        return Err(UiScriptError::ScriptTooLarge {
            len: bytes.len(),
            limit: limits.max_script_bytes,
        });
    }
    let mut scan = Scanner {
        bytes,
        limits,
        sites: Vec::new(),
        other_heads: BTreeMap::<String, u32>::new(),
        depth: 0,
        block: String::new(),
        at: 0,
    };
    scan.run()?;
    Ok(UiProgramScan {
        spelling: spelling.into(),
        sites: scan.sites,
        other_call_heads: scan
            .other_heads
            .into_iter()
            .map(|(head, sites)| OtherCallHead { head, sites })
            .collect(),
    })
}

struct Scanner<'a> {
    bytes: &'a [u8],
    limits: UiScriptLimits,
    sites: Vec<HostCallSite>,
    other_heads: BTreeMap<String, u32>,
    depth: usize,
    block: String,
    at: usize,
}

impl<'a> Scanner<'a> {
    fn run(&mut self) -> Result<(), UiScriptError> {
        let mut in_string = false;
        while self.at < self.bytes.len() {
            let byte = self.bytes[self.at];
            if in_string {
                // Inside a `"…"` literal a backslash escapes the next byte: the
                // measured corpus relies on it for `assets\\scripts\\`. Nothing
                // inside a literal is scanned for a call, so a literal that
                // spells `callback(` cannot invent a site.
                self.at += match byte {
                    b'\\' => 2,
                    b'"' => {
                        in_string = false;
                        1
                    }
                    _ => 1,
                };
                continue;
            }
            match byte {
                b'"' => {
                    in_string = true;
                    self.at += 1;
                }
                b'{' => self.open_block(),
                b'}' => {
                    self.depth = self.depth.saturating_sub(1);
                    self.at += 1;
                }
                _ => {
                    if let Some(head) = self.call_head() {
                        match head.as_str() {
                            "callback" => self.scan_call(DispatchForm::Callback, self.at)?,
                            "mail" => self.scan_call(DispatchForm::Mail, self.at)?,
                            _ => {
                                *self.other_heads.entry(head).or_default() += 1;
                                self.at += 1;
                            }
                        }
                    } else {
                        self.at += 1;
                    }
                }
            }
        }
        if self.depth != 0 {
            return Err(UiScriptError::UnbalancedBlocks { opened: self.depth });
        }
        Ok(())
    }

    /// `{` opens a block. At depth 0 the identifier scanned back from the brace
    /// becomes the enclosing block label of every site inside it; deeper braces
    /// only raise the depth.
    fn open_block(&mut self) {
        if self.depth == 0 {
            self.block = self.label_before(self.at).unwrap_or_default();
        }
        self.depth += 1;
        self.at += 1;
    }

    /// The identifier immediately before `offset`, skipping whitespace.
    fn label_before(&self, offset: usize) -> Option<String> {
        let mut end = offset;
        while end > 0 && is_space(self.bytes[end - 1]) {
            end -= 1;
        }
        let mut start = end;
        while start > 0 && is_ident_byte(self.bytes[start - 1]) {
            start -= 1;
        }
        if start == end {
            return None;
        }
        let label = String::from_utf8_lossy(&self.bytes[start..end]).into_owned();
        if label.len() > MAX_BLOCK_LABEL_BYTES {
            return None;
        }
        Some(label)
    }

    /// The call head at `self.at`, when the bytes there spell an identifier
    /// followed — after whitespace — by `(`. A head that is the tail of a longer
    /// identifier or of a member path is not a head.
    fn call_head(&self) -> Option<String> {
        if self.at > 0 {
            let prev = self.bytes[self.at - 1];
            if is_ident_byte(prev) || prev == b'.' {
                return None;
            }
        }
        let first = self.bytes[self.at];
        if !first.is_ascii_alphabetic() && first != b'_' {
            return None;
        }
        let mut end = self.at;
        while end < self.bytes.len() && is_ident_byte(self.bytes[end]) {
            end += 1;
        }
        let mut after = end;
        while after < self.bytes.len() && is_space(self.bytes[after]) {
            after += 1;
        }
        if after >= self.bytes.len() || self.bytes[after] != b'(' {
            return None;
        }
        Some(String::from_utf8_lossy(&self.bytes[self.at..end]).into_owned())
    }

    /// Scans the call whose head starts at `head_start` and records one
    /// [`HostCallSite`].
    fn scan_call(&mut self, form: DispatchForm, head_start: usize) -> Result<(), UiScriptError> {
        if self.sites.len() >= self.limits.max_sites {
            return Err(UiScriptError::TooManySites {
                limit: self.limits.max_sites,
            });
        }
        let mut open = head_start;
        while open < self.bytes.len() && self.bytes[open] != b'(' {
            open += 1;
        }
        let exprs = self.arguments(open)?;
        let block = self.block.clone();
        let span = ByteSpan::new(head_start as u64, (self.at - head_start) as u64);
        // The two measured forms put their expressions in different orders, and
        // that difference is measured rather than normalized away:
        //
        // * `callback($$target$$, <call>, <arg>…)` — a `$$…$$` reference, then
        //   the dispatch value, then the arguments;
        // * `mail(<message>, <recipient>)` — the dispatch value, then the
        //   arguments.
        //
        // A site whose first expression does not fit its form's shape is still
        // measured: the dispatch value is `None` and the expression is recorded
        // as an argument, so a variant of the form cannot be silently reshaped
        // into the common one.
        let mut target = None;
        let mut dispatch_at = 0usize;
        if form == DispatchForm::Callback
            && let Some(first) = exprs.first()
            && target_is_reference(self.bytes, first)
        {
            target = Some(classify(self.bytes, first));
            dispatch_at = 1;
        }
        let dispatch = exprs.get(dispatch_at);
        let native_id = dispatch.and_then(|expr| integer_literal(self.bytes, expr.clone()));
        let skip_target = target.is_some();
        let shapes = exprs
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != dispatch_at && !(skip_target && *index == 0))
            .map(|(_, expr)| classify(self.bytes, expr))
            .collect::<Vec<_>>();
        self.sites.push(HostCallSite {
            form,
            block,
            native_id,
            target,
            args: shapes,
            span,
        });
        Ok(())
    }

    /// Splits the argument list whose `(` is at `open`, returning each argument
    /// as a byte range and leaving `self.at` just past the closing `)`.
    fn arguments(&mut self, open: usize) -> Result<Vec<std::ops::Range<usize>>, UiScriptError> {
        // `at` starts just past the opening parenthesis, so the `)` that closes
        // the list is met at depth 0.
        let mut depth = 0usize;
        let mut at = open + 1;
        let mut start = open + 1;
        let mut args: Vec<std::ops::Range<usize>> = Vec::new();
        let mut in_string = false;
        while at < self.bytes.len() {
            let byte = self.bytes[at];
            if in_string {
                at += match byte {
                    b'\\' => 2,
                    b'"' => {
                        in_string = false;
                        1
                    }
                    _ => 1,
                };
                continue;
            }
            match byte {
                b'"' => {
                    in_string = true;
                    at += 1;
                }
                b'(' | b'[' | b'{' => {
                    depth += 1;
                    at += 1;
                }
                b')' | b']' | b'}' => {
                    if byte == b')' && depth == 0 {
                        if !self.bytes[start..at].iter().all(|b| is_space(*b)) {
                            args.push(start..at);
                        }
                        self.at = at + 1;
                        return Ok(args);
                    }
                    depth = depth.saturating_sub(1);
                    at += 1;
                }
                b',' if depth == 0 => {
                    args.push(start..at);
                    if args.len() > self.limits.max_args {
                        return Err(UiScriptError::TooManyArguments {
                            at: start as u64,
                            limit: self.limits.max_args,
                        });
                    }
                    start = at + 1;
                    at += 1;
                }
                _ => at += 1,
            }
        }
        Err(UiScriptError::UnterminatedCall { at: open as u64 })
    }
}

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

/// Whether an expression is the `$$…$$` reference the `callback` form's first
/// position holds. Only the two delimiters are checked; the name itself is
/// never retained.
fn target_is_reference(bytes: &[u8], expr: &std::ops::Range<usize>) -> bool {
    let Ok(text) = std::str::from_utf8(&bytes[expr.clone()]) else {
        return false;
    };
    let text = text.trim();
    text.len() >= 4 && text.starts_with("$$") && text.ends_with("$$")
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The integer a measured expression spells, when it spells one.
///
/// The bytes are inspected and discarded: no expression text is ever retained
/// by this module.
fn integer_literal(bytes: &[u8], expr: std::ops::Range<usize>) -> Option<i64> {
    let text = std::str::from_utf8(&bytes[expr.clone()]).ok()?;
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<i64>().ok()
}

/// The measured class of one argument expression.
///
/// Classification reads the bytes and keeps only the class. `Unevaluated`
/// covers every form this slice deliberately does not evaluate, and no
/// expression's text leaves this function.
fn classify(bytes: &[u8], expr: &std::ops::Range<usize>) -> ArgShape {
    let raw = &bytes[expr.clone()];
    if raw.len() > MAX_ARG_EXPR_BYTES {
        // A call carrying an over-long expression is refused by
        // `arguments`; a direct call cannot reach here with a short bound, and
        // an over-long expression is never evaluated.
        return ArgShape::Unevaluated;
    }
    let Ok(text) = std::str::from_utf8(raw) else {
        return ArgShape::Unevaluated;
    };
    let text = text.trim();
    if text.is_empty() {
        return ArgShape::Unevaluated;
    }
    if integer_literal(bytes, expr.clone()).is_some() {
        return ArgShape::IntegerLiteral;
    }
    if text.starts_with('"') && text.ends_with('"') && text.len() >= 2 {
        return ArgShape::StringLiteral;
    }
    if text.starts_with("$$") && text.ends_with("$$") && text.len() >= 4 {
        return ArgShape::NameRef;
    }
    if text.starts_with('@') && text.matches('@').count() >= 2 {
        return ArgShape::WidgetClassRef;
    }
    if text.contains('[') || text.contains(']') {
        return ArgShape::IndexedRef;
    }
    if is_ident_byte(text.as_bytes()[0])
        && is_ident_byte(text.as_bytes()[text.len() - 1])
        && text.chars().all(|c| is_ident_byte(c as u8) || c == '.')
    {
        return if text.contains('.') {
            ArgShape::MemberRef
        } else {
            ArgShape::NameRef
        };
    }
    // A bare floating-point literal is the one remaining shape this slice can
    // classify without evaluating; everything else (a parenthesised expression,
    // an arithmetic combination, a concatenation) stays `Unevaluated`.
    if text
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+' | b'e' | b'E'))
        && text.bytes().any(|b| b == b'.')
    {
        return ArgShape::FloatLiteral;
    }
    ArgShape::Unevaluated
}

// --- the measured corpus -----------------------------------------------------

/// One UI script program to measure, named by its container spelling.
#[derive(Clone, Copy, Debug)]
pub struct CorpusMember<'a> {
    /// The member's spelling inside its container (`ASSETS/SCRIPTS/KEYS.SCRIPT`).
    pub spelling: &'a str,
    /// The decoded program bytes.
    pub bytes: &'a [u8],
}

/// Per-argument-position shape counts of one observed dispatch id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ArgShapeCounts {
    counts: [u32; ArgShape::ALL.len()],
}

impl ArgShapeCounts {
    /// How many sites spelled this argument position as `shape`.
    pub fn count(self, shape: ArgShape) -> u32 {
        self.counts[shape.code() as usize]
    }

    /// How many sites carried this argument position at all.
    pub fn total(self) -> u32 {
        self.counts.iter().sum()
    }

    /// The most frequent class at this position, or `None` when no site carried
    /// the position. Ties resolve to the **first** shape in the
    /// [`ArgShape::ALL`] order, so the answer does not depend on iteration
    /// order. The shape itself carries no meaning, and a caller that needs one
    /// must still check [`ArgShapeCounts::is_uniform`].
    pub fn dominant(self) -> Option<ArgShape> {
        let mut best: Option<(ArgShape, u32)> = None;
        for shape in ArgShape::ALL {
            let count = self.count(shape);
            if count > best.map_or(0, |(_, top)| top) {
                best = Some((shape, count));
            }
        }
        best.map(|(shape, _)| shape)
    }

    /// True when every site spelled this position the same way.
    pub fn is_uniform(self) -> bool {
        self.total() > 0
            && ArgShape::ALL
                .iter()
                .all(|s| self.count(*s) == 0 || self.count(*s) == self.total())
    }

    fn add(&mut self, shape: ArgShape) {
        self.counts[shape.code() as usize] += 1;
    }
}

/// Every measured site of one dispatch form and one dispatch value.
#[derive(Clone, Debug, PartialEq)]
pub struct ObservedHostCall {
    /// The dispatch value the sites spell as an integer literal.
    pub native_id: i64,
    /// The dispatch form every site of this record was found in.
    pub form: DispatchForm,
    /// Sites measured across the corpus.
    pub sites: u32,
    /// How many distinct programs contributed a site.
    pub scripts: u32,
    /// Observed argument counts (excluding the dispatch expression), ascending.
    pub arities: Vec<u32>,
    /// Shape counts per argument position, in position order.
    pub arg_shapes: Vec<ArgShapeCounts>,
    /// The first site measured, as a container span.
    pub first_site: ByteSpan,
    /// The program the first site was measured in.
    pub first_spelling: String,
    /// How this was measured, and what that does and does not establish.
    pub evidence: ScriptEvidence,
}

/// Everything the corpus of UI script programs measured.
#[derive(Clone, Debug, PartialEq)]
pub struct HostCallCorpus {
    /// Programs measured.
    pub members: u32,
    /// Host-call sites measured across them.
    pub sites: u32,
    /// Sites whose dispatch expression is an integer literal.
    pub sites_with_native_id: u32,
    /// Sites whose dispatch expression is some other expression. They are
    /// counted, never folded into an id.
    pub sites_without_native_id: u32,
    /// One record per distinct dispatch form and dispatch value, in
    /// (form, native_id) order.
    pub calls: Vec<ObservedHostCall>,
    /// Call-shaped heads outside the two measured dispatch forms, in spelling
    /// order with their counts: the batch's measured boundary.
    pub other_call_heads: Vec<OtherCallHead>,
}

impl HostCallCorpus {
    /// Distinct dispatch values measured.
    pub fn call_count(&self) -> usize {
        self.calls.len()
    }

    /// The record for one dispatch form and value.
    pub fn call(&self, form: DispatchForm, native_id: i64) -> Option<&ObservedHostCall> {
        self.calls
            .iter()
            .find(|c| c.form == form && c.native_id == native_id)
    }

    /// Sites of call-shaped heads outside this batch's two dispatch forms.
    pub fn other_call_sites(&self) -> u32 {
        self.other_call_heads.iter().map(|h| h.sites).sum()
    }
}

/// Measures the host-call corpus of a set of UI script programs.
///
/// Every site is recorded with its program spelling and byte span, and every
/// record carries [`ScriptEvidence`] naming the program it was first measured
/// in. Nothing is written anywhere and no original text is retained.
///
/// # Errors
///
/// The first [`UiScriptError`] in member order: a member over a bound, an
/// unterminated call, an over-long argument expression or unbalanced blocks. A
/// refused program is never counted, so the corpus never reports a partial
/// member as a whole one.
pub fn measure_host_call_corpus<'a>(
    members: impl IntoIterator<Item = CorpusMember<'a>>,
    limits: UiScriptLimits,
) -> Result<HostCallCorpus, UiScriptError> {
    let mut members_count = 0u32;
    let mut sites = 0u32;
    let mut with_id = 0u32;
    let mut without_id = 0u32;
    let mut other_heads: BTreeMap<String, u32> = BTreeMap::new();
    // (form, native_id) -> accumulator
    let mut acc: BTreeMap<(DispatchForm, i64), Accumulator> = BTreeMap::new();

    for member in members {
        let scan = scan_ui_program(member.spelling, member.bytes, limits)?;
        members_count += 1;
        sites += scan.sites.len() as u32;
        for head in &scan.other_call_heads {
            *other_heads.entry(head.head.clone()).or_default() += head.sites;
        }
        for site in &scan.sites {
            match site.native_id {
                Some(native_id) => {
                    with_id += 1;
                    let entry = acc
                        .entry((site.form, native_id))
                        .or_insert_with(|| Accumulator {
                            spelling: member.spelling.to_owned(),
                            first_site: site.span,
                            scripts: BTreeMap::new(),
                            arities: BTreeMap::new(),
                            arg_shapes: Vec::new(),
                        });
                    *entry.scripts.entry(member.spelling.to_owned()).or_default() += 1;
                    *entry.arities.entry(site.args.len() as u32).or_default() += 1;
                    let widths = entry.arg_shapes.len();
                    for (index, shape) in site.args.iter().enumerate() {
                        if index < widths {
                            entry.arg_shapes[index].add(*shape);
                        } else {
                            entry.arg_shapes.push(ArgShapeCounts::default());
                            entry.arg_shapes[index].add(*shape);
                        }
                    }
                }
                None => without_id += 1,
            }
        }
    }

    let mut calls = Vec::with_capacity(acc.len());
    for ((form, native_id), entry) in acc {
        let note = format!(
            "{} site(s) of `{}` spelling {} measured as a shape in {} program(s); arities \
             {:?}; no meaning is claimed for the id",
            entry.sites(),
            form.label(),
            native_id,
            entry.scripts.len(),
            entry.arities.keys().copied().collect::<Vec<_>>()
        );
        let evidence = ScriptEvidence::new(
            ResearchMethod::StructuralDecode,
            Confidence::ObservedTool,
            EvidenceLocator::ContainerSpan {
                container: entry.spelling.clone(),
                span: entry.first_site,
            },
            note,
        )?;
        calls.push(ObservedHostCall {
            native_id,
            form,
            sites: entry.sites(),
            scripts: entry.scripts.len() as u32,
            arities: entry.arities.keys().copied().collect(),
            arg_shapes: entry.arg_shapes,
            first_site: entry.first_site,
            first_spelling: entry.spelling,
            evidence,
        });
    }

    Ok(HostCallCorpus {
        members: members_count,
        sites,
        sites_with_native_id: with_id,
        sites_without_native_id: without_id,
        calls,
        other_call_heads: other_heads
            .into_iter()
            .map(|(head, sites)| OtherCallHead { head, sites })
            .collect(),
    })
}

struct Accumulator {
    spelling: String,
    first_site: ByteSpan,
    scripts: BTreeMap<String, u32>,
    arities: BTreeMap<u32, u32>,
    arg_shapes: Vec<ArgShapeCounts>,
}

impl Accumulator {
    fn sites(&self) -> u32 {
        self.scripts.values().sum()
    }
}
