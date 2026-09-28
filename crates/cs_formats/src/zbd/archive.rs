//! The bounded container layer every ZBD family reader shares.
//!
//! Spec F06's deliverable asks for "distinct sound, reader, texture, interp,
//! GameZ and animation readers. **Each reader reports consumed ranges and
//! unsupported records**", and non-negotiable #2/#3 require that sound and
//! reader entries keep their declared metadata and source spans, that duplicate
//! entry names and numeric ids survive, and that a structural parse is never
//! advertised as a semantic interpretation. This module is the part of that
//! contract which does **not** depend on a family's member-index layout:
//!
//! * [`MemberExtent`] / [`MemberTable`] — one member as the container's own
//!   index declares it. The index is an **input**, not an assumption: F06-B
//!   claims no knowledge of where inside a reader or sound archive that index
//!   lives or how a name, id and extent are laid out inside it, because the
//!   layout was not documented when F06-B landed (spec F06 "Research boundary").
//!   Task #340 has since recorded it — a version-one table at the end of the
//!   file — and task #343 reads it ([`crate::zbd::trailer`]), producing the
//!   extents this type takes; the table stays an input so the bounds layer
//!   does not depend on any one index layout.
//!   The family beside that index is never the caller's word either: a family
//!   an observed role names comes from the two-key dispatch
//!   ([`MemberTable::from_dispatch`]), and a family no key names yet can only be
//!   named directly ([`MemberTable::named`], which refuses the first kind).
//! * [`require_family`] — the gate a family reader puts in front of its own
//!   bytes. A container whose family data belongs to another family fails with
//!   [`FamilyMismatch`] instead of being read by a parser that does not fit
//!   (spec F06 AC02: never fall back to another parser silently).
//! * [`list_members`] — the bounded listing: every declared member is checked
//!   against the container's length with checked arithmetic, an invalid member
//!   is recorded with its own [`MemberError`] while its valid siblings stay
//!   readable (non-negotiable #4), and the listing reports the ranges it
//!   consumed and the bytes no member claimed.
//!
//! Everything here runs through [`ParseContext::parse`], so the member table is
//! booked against the parse's allocation budget before its `Vec` exists, a
//! refused table costs no allocation, and a failed attempt leaves the ledger as
//! it found it so the same bytes can be retried honestly (spec F03, stages F03-B
//! and F03-C). Member content is **borrowed** from the container bytes, never
//! copied.

use std::fmt;

use cs_types::evidence::SourceSpan;

use crate::error::ParseError;
use crate::io::ParseContext;

use super::dispatch::{DispatchBasis, HeaderStatus, ZbdDispatch};
use super::family::{ZbdFamily, ZbdReaderId, family_record};
use super::header::HeaderRule;

/// Error scope stamped onto failures raised inside [`list_members`].
///
/// This is the one byte-level entrypoint of the family readers: the readers
/// themselves read no bytes of their own, they gate the family and shape the
/// listing, so there is no second scope to add (spec F03-C, "every parser
/// entrypoint names itself").
pub const CONTAINER_ENTRYPOINT: &str = "zbd.container";

/// Bytes one row of a member listing occupies.
///
/// The listing books `members.len() * MEMBER_ROW_BYTES` against the parse's
/// allocation budget before the row table exists, so the charge describes the
/// memory the rows really occupy rather than a guess.
pub const MEMBER_ROW_BYTES: u64 = size_of::<MemberRow<'static>>() as u64;

/// Bytes one entry of the derived range list occupies.
///
/// The range list holds at most one [`SourceSpan`] per declared member and is
/// charged separately from the row table so both allocations are inside the
/// budget.
pub const SOURCE_SPAN_BYTES: u64 = size_of::<SourceSpan>() as u64;

/// Why [`MemberTable::named`] reports no dispatch basis.
///
/// Used when the caller names the family the reader implements because **no**
/// dispatch key can name it yet. The sound family is the case F06-A recorded:
/// no `.zbd` basename in committed evidence is tied to sound bytes, so
/// `ZbdFamily::Sound` has no role rule and can only be reached by the sound
/// reader's own caller.
const NO_DISPATCH_BASIS: &str =
    "no dispatch key names this family yet; the caller named the family its reader implements";

/// Why a caller-named family's bytes are marked unvalidated even when its
/// header layout *is* documented: naming a family is not validating it.
const NAMED_NOT_VALIDATED: &str =
    "the caller named this family directly; no dispatch validated its header bytes";

/// Why a family the two-key dispatch already routes may not be named by a
/// caller instead.
const ROUTED_FAMILY: &str = "an observed installation role already names this family, so its member index must be built \
     from a dispatch: only that path checks the role and any documented header";

/// A caller tried to name a family the two-key dispatch already routes.
///
/// The member index of such a family has to be built with
/// [`MemberTable::from_dispatch`], so the family is decided by the two keys
/// (observed role, and a documented header where one exists) instead of by
/// assertion. Naming it instead would be the one way to get a reader to read
/// bytes of a family it does not implement **without** the reader ever being
/// able to see that they are of another family, which is exactly the silent
/// fallback spec F06 AC02 rules out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutableFamily {
    container: String,
    family: ZbdFamily,
    source: &'static str,
}

impl RoutableFamily {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        "family_routable_by_dispatch"
    }

    /// The container label the refused call carried.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// The family the caller tried to name.
    pub const fn family(&self) -> ZbdFamily {
        self.family
    }

    /// The inventory citation for why the family is decided by dispatch.
    pub const fn source(&self) -> &'static str {
        self.source
    }
}

impl fmt::Display for RoutableFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: the `{}` family cannot be named by the caller: {ROUTED_FAMILY}",
            self.container,
            self.family.as_str(),
        )
    }
}

impl std::error::Error for RoutableFamily {}

/// One member as the container's own index declares it.
///
/// `name` and `id` are kept verbatim: names are bytes (no UTF-8 assumption, no
/// normalization) and duplicate names or ids are **rows**, never merged
/// (spec F06 non-negotiable #3). `span` is the contract's
/// [`SourceSpan`]: the member's place in the container, which
/// [`ArchiveListing::member_bytes`] bounds before it hands anything out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemberExtent<'a> {
    name: &'a [u8],
    id: Option<u32>,
    span: SourceSpan,
}

impl<'a> MemberExtent<'a> {
    /// Records one declared member.
    ///
    /// `id` is `None` when the family's index carries no numeric id for this
    /// member; that is different from the id `0`, which is preserved as
    /// `Some(0)`.
    pub const fn new(name: &'a [u8], id: Option<u32>, span: SourceSpan) -> Self {
        Self { name, id, span }
    }

    /// The member's name bytes exactly as declared.
    pub const fn name(&self) -> &'a [u8] {
        self.name
    }

    /// The member's numeric id, when its index declares one.
    pub const fn id(&self) -> Option<u32> {
        self.id
    }

    /// Where the member claims to live inside the container.
    pub const fn span(&self) -> SourceSpan {
        self.span
    }
}

/// How the family of a member table was decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FamilyOrigin {
    /// F06-A's two-key dispatch decided it, on this basis.
    Dispatched {
        /// Which key identified the family
        /// (`crate::zbd::dispatch::DispatchBasis`).
        basis: DispatchBasis,
    },
    /// No dispatch key names this family yet, so the caller named the family
    /// the reader implements. `header_status` is then
    /// [`HeaderStatus::Unvalidated`]: naming a family is not validating its
    /// bytes.
    NamedByCaller,
}

impl FamilyOrigin {
    /// Why the family was decided this way, for a diagnostic.
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Dispatched { .. } => "the two-key dispatch identified the family",
            Self::NamedByCaller => NO_DISPATCH_BASIS,
        }
    }

    /// Whether a dispatch key identified the family.
    pub const fn is_dispatched(self) -> bool {
        matches!(self, Self::Dispatched { .. })
    }
}

/// The member index of one container, as its family's own reader declares it,
/// plus the evidence for the container's family.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberTable<'a> {
    container: String,
    family: ZbdFamily,
    origin: FamilyOrigin,
    header_status: HeaderStatus,
    members: &'a [MemberExtent<'a>],
}

impl<'a> MemberTable<'a> {
    /// The member index of a container F06-A's dispatch already routed.
    ///
    /// The dispatch decides are kept verbatim — container label, family, basis
    /// and header status — so a reader's result can always be traced back to
    /// the two keys that identified the container and can never claim more
    /// validation than dispatch established.
    pub fn from_dispatch(dispatch: &ZbdDispatch<'_>, members: &'a [MemberExtent<'a>]) -> Self {
        Self {
            container: dispatch.container().to_owned(),
            family: dispatch.family(),
            origin: FamilyOrigin::Dispatched {
                basis: dispatch.basis(),
            },
            header_status: dispatch.header_status(),
            members,
        }
    }

    /// The member index of a container **no** dispatch key names yet.
    ///
    /// Since task #340 every family owns an observed role rule (the sound
    /// family's is `ZBD/sounds*.zbd`), so this refuses every family today; it
    /// stays as the guard for a family a later stage adds before its archive
    /// names are observed. The header status is [`HeaderStatus::Unvalidated`] with the family's own
    /// recorded reason, so the reader still cannot pretend it checked bytes.
    ///
    /// A family an observed installation role **does** name is refused with
    /// [`RoutableFamily`]: it has to come from [`Self::from_dispatch`], which
    /// is the only path that checks the role and any documented header. Letting
    /// a caller name such a family would let it hand another family's bytes to
    /// a reader that never looks at them — the silent fallback spec F06 AC02
    /// forbids, and one no later evidence could detect.
    ///
    /// # Errors
    ///
    /// [`RoutableFamily`] when the family owns an observed role rule, i.e. when
    /// dispatch can name it.
    pub fn named(
        container: impl Into<String>,
        family: ZbdFamily,
        members: &'a [MemberExtent<'a>],
    ) -> Result<Self, RoutableFamily> {
        let record = family_record(family);
        if !record.role_rules().is_empty() {
            return Err(RoutableFamily {
                container: container.into(),
                family,
                source: record.source(),
            });
        }
        let header_status = match record.header_rule() {
            HeaderRule::Undocumented { reason } => HeaderStatus::Unvalidated { reason },
            // A family can have a documented header and still own no role rule
            // (none does today); naming it is allowed, and still validates
            // nothing, because no dispatch looked at its bytes.
            HeaderRule::Signature(_) => HeaderStatus::Unvalidated {
                reason: NAMED_NOT_VALIDATED,
            },
        };
        Ok(Self {
            container: container.into(),
            family,
            origin: FamilyOrigin::NamedByCaller,
            header_status,
            members,
        })
    }

    /// Provenance label of the container these members belong to.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// The family the container was decided to be.
    pub const fn family(&self) -> ZbdFamily {
        self.family
    }

    /// How the family was decided.
    pub const fn origin(&self) -> FamilyOrigin {
        self.origin
    }

    /// What dispatch established about the container's bytes.
    pub const fn header_status(&self) -> HeaderStatus {
        self.header_status
    }

    /// The declared members, in index order.
    pub const fn members(&self) -> &'a [MemberExtent<'a>] {
        self.members
    }

    /// Number of declared members.
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Whether the index declares no member.
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

/// Why one declared member cannot be handed out of its container.
///
/// The variants are per-member and non-fatal on purpose: spec F06
/// non-negotiable #4 requires a diagnostic listing to be able to continue past
/// an invalid member and show every error, so these are recorded on the
/// member's [`MemberRow`] instead of ending the whole listing. They carry
/// counts and offsets only, never member bytes (spec F03: errors carry metadata
/// only, so a diagnostic cannot echo private installation data).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberError {
    /// `offset + length` overflows `u64`, so the member's extent cannot even
    /// be described.
    ExtentOverflow {
        /// Index of the offending member in the table.
        index: usize,
        /// Declared offset.
        offset: u64,
        /// Declared length.
        length: u64,
    },
    /// The member's extent ends past the end of the container.
    OutOfBounds {
        /// Index of the offending member in the table.
        index: usize,
        /// Declared offset.
        offset: u64,
        /// Declared length.
        length: u64,
        /// Bytes the container actually has.
        container_len: u64,
        /// The exclusive end offset the member claimed.
        end: u64,
    },
}

impl MemberError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ExtentOverflow { .. } => "extent_overflow",
            Self::OutOfBounds { .. } => "member_out_of_bounds",
        }
    }

    /// Index of the member this error belongs to.
    pub const fn member_index(&self) -> usize {
        match self {
            Self::ExtentOverflow { index, .. } | Self::OutOfBounds { index, .. } => *index,
        }
    }

    /// Offset of the member this error belongs to.
    pub const fn offset(&self) -> u64 {
        match self {
            Self::ExtentOverflow { offset, .. } | Self::OutOfBounds { offset, .. } => *offset,
        }
    }
}

impl fmt::Display for MemberError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExtentOverflow {
                index,
                offset,
                length,
            } => write!(
                f,
                "member {index} at offset {offset}: its declared length {length} overflows \
                 offset + length in u64"
            ),
            Self::OutOfBounds {
                index,
                offset,
                length,
                container_len,
                end,
            } => write!(
                f,
                "member {index} at offset {offset}: its declared extent of {length} bytes ends \
                 at {end}, past the {container_len}-byte container"
            ),
        }
    }
}

impl std::error::Error for MemberError {}

/// What the listing could establish about one declared member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberStatus {
    /// The member's extent lies inside the container, so its bytes can be
    /// handed out.
    Readable,
    /// The member could not be handed out, for this recorded reason.
    Failed(MemberError),
}

/// One row of a listing: the member exactly as declared plus what the bounds
/// check established about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemberRow<'a> {
    index: usize,
    name: &'a [u8],
    id: Option<u32>,
    span: SourceSpan,
    status: MemberStatus,
}

impl<'a> MemberRow<'a> {
    /// Position of this member in the declared index (`0..members.len()`).
    pub const fn index(&self) -> usize {
        self.index
    }

    /// The member's name bytes exactly as declared.
    pub const fn name(&self) -> &'a [u8] {
        self.name
    }

    /// The member's numeric id, when its index declares one.
    pub const fn id(&self) -> Option<u32> {
        self.id
    }

    /// The extent the member declared, whether or not it is inside the
    /// container.
    pub const fn span(&self) -> SourceSpan {
        self.span
    }

    /// What the bounds check established.
    pub const fn status(&self) -> MemberStatus {
        self.status
    }

    /// Whether this member's bytes can be handed out.
    pub const fn is_readable(&self) -> bool {
        matches!(self.status, MemberStatus::Readable)
    }

    /// Why this member could not be handed out, when it could not.
    pub const fn error(&self) -> Option<MemberError> {
        match self.status {
            MemberStatus::Readable => None,
            MemberStatus::Failed(error) => Some(error),
        }
    }
}

/// The strict status of a listing (spec F06 AC04's status half).
///
/// A listing with failures is still useful for a diagnostic — that is the point
/// of per-member errors — but it never reports itself clean, so nothing can
/// read "some members failed" as "the container is fine".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerStatus {
    /// Every declared member lies inside the container.
    Clean,
    /// At least one member was refused; the count is the number of rows that
    /// failed.
    Failed {
        /// Members that could not be handed out.
        failures: usize,
    },
}

impl ContainerStatus {
    /// Whether every declared member was inside the container.
    pub const fn is_clean(self) -> bool {
        matches!(self, Self::Clean)
    }

    /// How many members failed.
    pub const fn failures(self) -> usize {
        match self {
            Self::Clean => 0,
            Self::Failed { failures } => failures,
        }
    }

    /// Stable label for reports and diagnostics.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Failed { .. } => "failed",
        }
    }
}

/// One record a reader could not support, with the span it occupies.
///
/// This is what F06's deliverable means by "unsupported records": a member the
/// reader listed but cannot interpret, kept with its identity and its place in
/// the container, never silently dropped (spec F06 non-negotiable #4 and the
/// IDENTITY-CONTENT contract: "collections cannot exclude failed entries").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnsupportedRecord<'a> {
    index: usize,
    name: &'a [u8],
    id: Option<u32>,
    span: SourceSpan,
    reason: &'static str,
}

impl<'a> UnsupportedRecord<'a> {
    /// Records the readable row `row` as unsupported for `reason`.
    pub const fn new(row: &MemberRow<'a>, reason: &'static str) -> Self {
        Self {
            index: row.index(),
            name: row.name(),
            id: row.id(),
            span: row.span(),
            reason,
        }
    }

    /// Position of the record in the declared index.
    pub const fn index(&self) -> usize {
        self.index
    }

    /// The record's name bytes exactly as declared.
    pub const fn name(&self) -> &'a [u8] {
        self.name
    }

    /// The record's numeric id, when its index declares one.
    pub const fn id(&self) -> Option<u32> {
        self.id
    }

    /// Where the record claims to live inside the container.
    pub const fn span(&self) -> SourceSpan {
        self.span
    }

    /// Why this stage cannot support the record.
    pub const fn reason(&self) -> &'static str {
        self.reason
    }
}

/// The bounded listing of one container: what every declared member is, which
/// of them can be handed out, which ranges the reader consumed and which bytes
/// of the container no member claimed.
///
/// Built only by [`list_members`], so a listing never mixes members from
/// different containers and never reports a member as readable unless its
/// extent was checked against this container's length.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveListing<'a> {
    container: String,
    family: ZbdFamily,
    bytes: &'a [u8],
    rows: Vec<MemberRow<'a>>,
    consumed: Vec<SourceSpan>,
    failures: usize,
}

impl<'a> ArchiveListing<'a> {
    /// Provenance label of the listed container.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// The family the container was decided to be.
    pub const fn family(&self) -> ZbdFamily {
        self.family
    }

    /// Bytes the container has.
    pub fn container_len(&self) -> u64 {
        self.bytes.len() as u64
    }

    /// Number of declared members, readable or not.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the container's index declares no member.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Every row, in declared order.
    pub fn rows(&self) -> &[MemberRow<'a>] {
        &self.rows
    }

    /// The row at `index`, or `None` when the index is out of range.
    pub fn row(&self, index: usize) -> Option<&MemberRow<'a>> {
        self.rows.get(index)
    }

    /// The bytes of member `index`, borrowed from the container.
    ///
    /// `None` when the index is out of range **or** when that member failed its
    /// bounds check: an out-of-bounds extent yields nothing at all, never a
    /// truncated read.
    pub fn member_bytes(&self, index: usize) -> Option<&'a [u8]> {
        let row = self.rows.get(index)?;
        if !row.is_readable() {
            return None;
        }
        let start = usize::try_from(row.span().offset).ok()?;
        let end = start.checked_add(usize::try_from(row.span().length).ok()?)?;
        self.bytes.get(start..end)
    }

    /// The ranges the readable members consumed, merged into as few spans as
    /// possible and sorted by offset.
    ///
    /// A member that failed its bounds check contributes nothing here: this is
    /// what a reader actually consumed, not what the index hoped for.
    pub fn consumed_ranges(&self) -> &[SourceSpan] {
        &self.consumed
    }

    /// The stretches of the container no readable member claimed, in order.
    ///
    /// Derived on demand from [`Self::consumed_ranges`], so the result holds at
    /// most one entry per gap and is never part of the parse's allocation
    /// charge. A member that failed its bounds check leaves its stretch inside
    /// the uncovered set, which is how a diagnostic sees the corruption.
    pub fn uncovered_ranges(&self) -> Vec<SourceSpan> {
        let mut uncovered = Vec::new();
        let mut cursor = 0u64;
        for span in &self.consumed {
            if span.offset > cursor {
                uncovered.push(SourceSpan {
                    offset: cursor,
                    length: span.offset - cursor,
                });
            }
            let end = span.offset.saturating_add(span.length);
            if end > cursor {
                cursor = end;
            }
        }
        if cursor < self.container_len() {
            uncovered.push(SourceSpan {
                offset: cursor,
                length: self.container_len() - cursor,
            });
        }
        uncovered
    }

    /// How many declared members failed their bounds check.
    pub fn failures(&self) -> usize {
        self.failures
    }

    /// The strict status of this listing.
    pub fn status(&self) -> ContainerStatus {
        if self.failures == 0 {
            ContainerStatus::Clean
        } else {
            ContainerStatus::Failed {
                failures: self.failures,
            }
        }
    }
}

/// Why a whole listing could not be produced.
///
/// Only structural failures of the parse itself are fatal here: the member
/// table exceeding the parse's allocation budget (spec F03 non-negotiable #2).
/// A member that does not fit its container is not an error of the listing but
/// a [`MemberError`] on that member's row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContainerError {
    /// A failure from the checked reader or one of its budgets, already scoped
    /// as `zbd.container.<field>` by [`list_members`].
    Parse(ParseError),
}

impl ContainerError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Parse(_) => "parse",
        }
    }

    /// The container label the failure came from.
    pub fn container(&self) -> &str {
        match self {
            Self::Parse(error) => &error.container,
        }
    }

    /// The byte offset the failure is anchored at.
    pub fn offset(&self) -> u64 {
        match self {
            Self::Parse(error) => error.offset,
        }
    }
}

impl From<ParseError> for ContainerError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl fmt::Display for ContainerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ContainerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
        }
    }
}

/// A container's family data is not the family the reader asked for.
///
/// Spec F06 AC02: "A valid header with incompatible family data fails
/// explicitly, never falls back to another parser silently." The failure
/// carries the header status the container *did* establish, so a diagnostic can
/// show that the header validated and the family still did not match — which is
/// the case an implicit fallback would hide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FamilyMismatch {
    container: String,
    expected: ZbdFamily,
    expected_reader: ZbdReaderId,
    actual: ZbdFamily,
    origin: FamilyOrigin,
    header_status: HeaderStatus,
}

impl FamilyMismatch {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        "family_mismatch"
    }

    /// The container label the failure came from.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// The family the reader implements.
    pub const fn expected(&self) -> ZbdFamily {
        self.expected
    }

    /// The reader slot the family routes to (the spec's reader vocabulary).
    pub const fn expected_reader(&self) -> ZbdReaderId {
        self.expected_reader
    }

    /// The family the container actually carries.
    pub const fn actual(&self) -> ZbdFamily {
        self.actual
    }

    /// How the container's family was decided.
    pub const fn origin(&self) -> FamilyOrigin {
        self.origin
    }

    /// What dispatch established about the container's header bytes.
    pub const fn header_status(&self) -> HeaderStatus {
        self.header_status
    }
}

impl fmt::Display for FamilyMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let HeaderStatus::Validated { signature, version } = self.header_status else {
            return write!(
                f,
                "{}: the container carries `{}` family data ({}) but the `{}` reader was asked \
                 to read it",
                self.container,
                self.actual.as_str(),
                self.origin.reason(),
                self.expected_reader.label()
            );
        };
        write!(
            f,
            "{}: header signature 0x{signature:08X} version {version} validated the `{}` \
             family, but the `{}` reader was asked to read it",
            self.container,
            self.actual.as_str(),
            self.expected_reader.label()
        )
    }
}

impl std::error::Error for FamilyMismatch {}

/// The gate a family reader puts in front of its own bytes.
///
/// `expected` is the family the calling reader implements. A container that
/// carries another family's data fails with [`FamilyMismatch`] and the caller
/// gets **no** listing: there is no path from this function to a reader other
/// than the one that was asked for.
///
/// # Errors
///
/// [`FamilyMismatch`] whenever [`MemberTable::family`] is not `expected`.
pub fn require_family(table: &MemberTable<'_>, expected: ZbdFamily) -> Result<(), FamilyMismatch> {
    if table.family() == expected {
        return Ok(());
    }
    Err(FamilyMismatch {
        container: table.container().to_owned(),
        expected,
        expected_reader: expected.reader(),
        actual: table.family(),
        origin: table.origin(),
        header_status: table.header_status(),
    })
}

/// The reason a family's layout is not documented, reported verbatim.
///
/// Readers quote this when they list a record they cannot support, so the
/// unknown keeps pointing at the research boundary that produced it instead of
/// at the reader.
pub fn undocumented_reason(family: ZbdFamily) -> &'static str {
    match family_record(family).header_rule() {
        HeaderRule::Undocumented { reason } => reason,
        HeaderRule::Signature(_) => NAMED_NOT_VALIDATED,
    }
}

/// Lists the members `table` declares, bounding every extent against `bytes`.
///
/// `bytes` is the whole container (what a mount or the VFS hands the reader)
/// and `table` is the member index as the family's own reader declares it; both
/// must outlive the listing, whose member content borrows from them.
///
/// The listing is produced in one pass through [`ParseContext::parse`]: the
/// row table and the derived range list are booked against the parse's
/// allocation budget *before* either `Vec` exists, and every extent is checked
/// with checked arithmetic before a row claims it is readable. A member that
/// fails is recorded on its row and the listing continues, so one corrupt
/// member does not hide its valid siblings (spec F06 non-negotiable #4).
///
/// # Errors
///
/// [`ContainerError::Parse`] when the member table does not fit the parse's
/// allocation budget. A member whose extent overflows or leaves the container
/// is **not** an error of this call: it becomes a [`MemberError`] on its row.
pub fn list_members<'a>(
    context: &mut ParseContext,
    bytes: &'a [u8],
    table: &'a MemberTable<'a>,
) -> Result<ArchiveListing<'a>, ContainerError> {
    let container = table.container().to_owned();
    let container_len = bytes.len() as u64;
    context
        .parse(
            CONTAINER_ENTRYPOINT,
            bytes,
            |_reader, allocation, _recursion| {
                // Charge both vectors before either exists: a hostile member count
                // costs a refusal, never an allocation.
                let members = table.len() as u64;
                allocation.reserve("members", 0, members, MEMBER_ROW_BYTES)?;
                allocation.reserve("member_ranges", 0, members, SOURCE_SPAN_BYTES)?;

                let mut rows = Vec::with_capacity(table.len());
                let mut consumed = Vec::with_capacity(table.len());
                let mut failures = 0usize;
                for (index, extent) in table.members().iter().enumerate() {
                    let span = extent.span();
                    let status = match check_extent(index, span, container_len) {
                        Ok(()) => {
                            consumed.push(span);
                            MemberStatus::Readable
                        }
                        Err(error) => {
                            failures += 1;
                            MemberStatus::Failed(error)
                        }
                    };
                    rows.push(MemberRow {
                        index,
                        name: extent.name(),
                        id: extent.id(),
                        span,
                        status,
                    });
                }
                merge_ranges(&mut consumed);
                Ok(ArchiveListing {
                    container: container.clone(),
                    family: table.family(),
                    bytes,
                    rows,
                    consumed,
                    failures,
                })
            },
        )
        .map_err(ContainerError::Parse)
}

/// Checks one declared extent against the container's length.
///
/// Both failure modes are per-member and non-fatal; `offset + length` is
/// checked before the end is compared, so a hostile extent cannot overflow a
/// comparison into a pass (spec F03 non-negotiable #2).
fn check_extent(index: usize, span: SourceSpan, container_len: u64) -> Result<(), MemberError> {
    let Some(end) = span.offset.checked_add(span.length) else {
        return Err(MemberError::ExtentOverflow {
            index,
            offset: span.offset,
            length: span.length,
        });
    };
    if end > container_len {
        return Err(MemberError::OutOfBounds {
            index,
            offset: span.offset,
            length: span.length,
            container_len,
            end,
        });
    }
    Ok(())
}

/// Sorts `ranges` by offset and merges overlapping or adjacent spans.
///
/// Extents reach this function only after [`check_extent`] accepted them, so
/// every end is inside the container and the saturating adds below cannot
/// differ from the checked ones; saturation is used anyway so a future caller
/// cannot make this panic.
fn merge_ranges(ranges: &mut Vec<SourceSpan>) {
    ranges.sort_by_key(|span| (span.offset, span.length));
    let mut merged: Vec<SourceSpan> = Vec::with_capacity(ranges.len());
    for span in ranges.iter() {
        match merged.last_mut() {
            Some(last) => {
                let last_end = last.offset.saturating_add(last.length);
                let end = span.offset.saturating_add(span.length);
                if span.offset <= last_end {
                    if end > last_end {
                        last.length = end - last.offset;
                    }
                } else {
                    merged.push(*span);
                }
            }
            None => merged.push(*span),
        }
    }
    *ranges = merged;
}
