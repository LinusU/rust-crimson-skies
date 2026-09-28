//! The reader-family reader: reader-archive entries with their byte content and
//! encoding evidence.
//!
//! Spec F06 non-negotiable #2: "Reader entries retain byte content and encoding
//! evidence." This stage implements the half of that which needs no field
//! offsets: [`read_reader_archive`] gates the family, lists the container's
//! declared members with bounds ([`crate::zbd::archive`]), and hands out each
//! entry's **verbatim** bytes with the evidence about its encoding.
//!
//! What it deliberately does not do: decode anything. The committed research
//! pack documents no encoding layout for the reader family — only the
//! `INTERP.ZBD` header is documented (`docs/research/FORMAT-NOTES.md`, "INTERP
//! observed subset" [S07]) — so every entry comes back as
//! [`EncodingEvidence::Undeclared`] carrying the family's own recorded reason.
//! Spec F06's research boundary requires the layout to be read from the pinned
//! source and checked against the installation first (task #340); until then
//! the entries are an inventory, not an interpretation, which is what
//! non-negotiable #3 ("archive structural parse success does not prove
//! semantic interpretation") demands.

use std::fmt;

use cs_types::evidence::{ClaimStatus, SourceSpan};

use super::archive::{
    ArchiveListing, ContainerError, ContainerStatus, FamilyMismatch, FamilyOrigin, MemberTable,
    UnsupportedRecord, list_members, require_family, undocumented_reason,
};
use super::dispatch::HeaderStatus;
use super::family::ZbdFamily;
use crate::io::ParseContext;

/// What is known about a reader entry's encoding.
///
/// One variant on purpose: the pack documents no reader encoding, so this stage
/// can only ever report that it does not know. A future stage that reads the
/// layout from the pinned source and checks it against the installation (#340)
/// adds the documented case here; until then `ClaimStatus::Unknown` is the
/// honest class and no entry may be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodingEvidence {
    /// No encoding layout for this family is documented, so the entry's bytes
    /// are retained verbatim and not decoded.
    Undeclared {
        /// Why the encoding is not known, quoted from the family inventory.
        reason: &'static str,
    },
}

impl EncodingEvidence {
    /// The evidence class this claim carries: `unknown`, by construction.
    pub const fn evidence(self) -> ClaimStatus {
        match self {
            Self::Undeclared { .. } => ClaimStatus::Unknown,
        }
    }

    /// The recorded reason this stage cannot decode the entry.
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Undeclared { reason } => reason,
        }
    }
}

/// One reader-archive entry: its identity, its source span, its verbatim byte
/// content and the evidence about its encoding.
///
/// `content` borrows the container bytes, so listing an archive copies nothing;
/// `name` stays raw bytes (no UTF-8 assumption, no normalization) and `id`
/// stays verbatim, including a duplicate of another entry's (spec F06
/// non-negotiable #3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReaderEntry<'a> {
    index: usize,
    name: &'a [u8],
    id: Option<u32>,
    span: SourceSpan,
    content: &'a [u8],
    encoding: EncodingEvidence,
}

impl<'a> ReaderEntry<'a> {
    /// Position of the entry in the container's declared index.
    pub const fn index(&self) -> usize {
        self.index
    }

    /// The entry's name bytes exactly as declared.
    pub const fn name(&self) -> &'a [u8] {
        self.name
    }

    /// The entry's numeric id, when its index declares one.
    pub const fn id(&self) -> Option<u32> {
        self.id
    }

    /// Where the entry lives inside the container.
    pub const fn span(&self) -> SourceSpan {
        self.span
    }

    /// The entry's bytes, exactly as the container stores them.
    pub const fn content(&self) -> &'a [u8] {
        self.content
    }

    /// What is known about the entry's encoding.
    pub const fn encoding(&self) -> EncodingEvidence {
        self.encoding
    }
}

/// A reader archive: the bounded listing plus this stage's reader entries.
///
/// The listing is the authority on structure (rows, consumed ranges, strict
/// status); the entries are the subset whose bytes could be handed out. Neither
/// claims more than the other: a clean listing still reports every entry as an
/// unsupported record, because a structural pass is not an interpretation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderArchive<'a> {
    listing: ArchiveListing<'a>,
    origin: FamilyOrigin,
    header_status: HeaderStatus,
}

impl<'a> ReaderArchive<'a> {
    /// Provenance label of the archive.
    pub fn container(&self) -> &str {
        self.listing.container()
    }

    /// The family this archive is: always [`ZbdFamily::Reader`], because the
    /// gate in [`read_reader_archive`] refuses anything else.
    pub const fn family(&self) -> ZbdFamily {
        ZbdFamily::Reader
    }

    /// How the container's family was decided.
    pub const fn origin(&self) -> FamilyOrigin {
        self.origin
    }

    /// What dispatch established about the container's header bytes.
    pub const fn header_status(&self) -> HeaderStatus {
        self.header_status
    }

    /// The bounded listing behind the entries.
    pub fn listing(&self) -> &ArchiveListing<'a> {
        &self.listing
    }

    /// Number of declared members, readable or not.
    pub fn len(&self) -> usize {
        self.listing.len()
    }

    /// Whether the container's index declares no member.
    pub fn is_empty(&self) -> bool {
        self.listing.is_empty()
    }

    /// The entry at `index`, or `None` when the index is out of range or that
    /// member failed its bounds check.
    pub fn entry(&self, index: usize) -> Option<ReaderEntry<'a>> {
        let row = self.listing.row(index)?;
        let content = self.listing.member_bytes(index)?;
        Some(ReaderEntry {
            index: row.index(),
            name: row.name(),
            id: row.id(),
            span: row.span(),
            content,
            encoding: EncodingEvidence::Undeclared {
                reason: undocumented_reason(ZbdFamily::Reader),
            },
        })
    }

    /// Every readable entry, in declared order.
    pub fn entries(&self) -> impl Iterator<Item = ReaderEntry<'a>> + '_ {
        (0..self.listing.len()).filter_map(|index| self.entry(index))
    }

    /// The ranges the reader consumed, merged and sorted.
    pub fn consumed_ranges(&self) -> &[SourceSpan] {
        self.listing.consumed_ranges()
    }

    /// The stretches of the container no readable member claimed.
    pub fn uncovered_ranges(&self) -> Vec<SourceSpan> {
        self.listing.uncovered_ranges()
    }

    /// How many declared members failed their bounds check.
    pub fn failures(&self) -> usize {
        self.listing.failures()
    }

    /// The strict status of the archive.
    pub fn status(&self) -> ContainerStatus {
        self.listing.status()
    }

    /// Every entry this stage cannot interpret, with its span.
    ///
    /// Today that is every readable entry, because no reader encoding layout is
    /// documented; the list shrinks as evidence lands. Failed members are not
    /// in it — they are already listed as failures on their rows.
    pub fn unsupported_records(&self) -> Vec<UnsupportedRecord<'a>> {
        self.listing
            .rows()
            .iter()
            .filter(|row| row.is_readable())
            .map(|row| UnsupportedRecord::new(row, undocumented_reason(ZbdFamily::Reader)))
            .collect()
    }
}

/// Why a reader archive could not be produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReaderError {
    /// The container carries another family's data; the reader-archive reader
    /// refuses it instead of reading it with a parser that does not fit (spec
    /// F06 AC02).
    Family(FamilyMismatch),
    /// The listing itself could not be produced.
    Container(ContainerError),
}

impl ReaderError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Family(mismatch) => mismatch.code(),
            Self::Container(error) => error.code(),
        }
    }

    /// The container label the failure came from.
    pub fn container(&self) -> &str {
        match self {
            Self::Family(mismatch) => mismatch.container(),
            Self::Container(error) => error.container(),
        }
    }
}

impl From<FamilyMismatch> for ReaderError {
    fn from(error: FamilyMismatch) -> Self {
        Self::Family(error)
    }
}

impl From<ContainerError> for ReaderError {
    fn from(error: ContainerError) -> Self {
        Self::Container(error)
    }
}

impl fmt::Display for ReaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Family(mismatch) => write!(f, "{mismatch}"),
            Self::Container(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ReaderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Family(mismatch) => Some(mismatch),
            Self::Container(error) => Some(error),
        }
    }
}

/// Reads the reader archive `table` declares inside `bytes`.
///
/// `bytes` is the whole container; `table` is its member index (see
/// [`MemberTable`] for why the index is an input in this stage). The family gate
/// runs first: a container of another family — including one whose header
/// **validated** for that other family — produces
/// [`ReaderError::Family`] and no archive at all, which is the explicit failure
/// spec F06 AC02 requires instead of a silent fallback to another parser.
///
/// The listing runs through [`ParseContext::parse`], so the member table is
/// booked against the parse's allocation budget and a refused table leaves the
/// ledger untouched, so the same bytes can be retried with a bigger budget.
///
/// # Errors
///
/// [`ReaderError::Family`] when `table` is not a [`ZbdFamily::Reader`] index, and
/// [`ReaderError::Container`] when the member table does not fit the parse's
/// allocation budget.
pub fn read_reader_archive<'a>(
    context: &mut ParseContext,
    table: &'a MemberTable<'a>,
    bytes: &'a [u8],
) -> Result<ReaderArchive<'a>, ReaderError> {
    require_family(table, ZbdFamily::Reader)?;
    let listing = list_members(context, bytes, table)?;
    Ok(ReaderArchive {
        listing,
        origin: table.origin(),
        header_status: table.header_status(),
    })
}
