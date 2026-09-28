//! The sound-family reader: sound entries with their source spans and their
//! declared sample descriptor.
//!
//! Spec F06 non-negotiable #2: "Sound entries retain sample format, channels,
//! rate, loop metadata if present, and source span." This stage implements the
//! half of that which needs no field offsets: [`read_sound_archive`] gates the
//! family, lists the container's declared members with bounds
//! ([`crate::zbd::archive`]), and hands out each entry's verbatim bytes with its
//! span and a descriptor whose fields report the recorded unknown.
//!
//! What it deliberately does not do: name a sample format, a channel count, a
//! rate or a loop point. Task #340 tied the family to `ZBD/sounds*.zbd` and found
//! that those archives have no leading header: the member table sits at the end
//! of the file and the members are RIFF/WAVE files (task #340 findings). Reading
//! that table (task #343) and each member's WAVE header (task #344) is still
//! to come. Until then every descriptor field
//! is [`SoundField::Unknown`] carrying the family's own recorded reason, so
//! nothing downstream can read a fabricated `22050 Hz` as a measured value.
//!
//! There is deliberately **no** `playable()`, `decoded()` or `samples()` accessor
//! on [`SoundArchive`]: with the descriptor unknown, any such method would be a
//! playability claim the bytes cannot support (spec F06 non-negotiable #4: a
//! listing "may continue and show every error without advertising playability").

use std::fmt;

use cs_types::evidence::SourceSpan;

use super::archive::{
    ArchiveListing, ContainerError, ContainerStatus, FamilyMismatch, FamilyOrigin, MemberTable,
    UnsupportedRecord, list_members, require_family, undocumented_reason,
};
use super::dispatch::HeaderStatus;
use super::family::ZbdFamily;
use crate::io::ParseContext;

/// One field of a sound entry's declared descriptor.
///
/// The two states the contract distinguishes
/// (`docs/contracts/IDENTITY-CONTENT.md`: "Unknown original units are
/// `Resolved::Unknown`, not assumed SI"): a value read from a documented
/// layout, or the recorded reason no such value is known. This stage can only
/// ever produce the second one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundField<T> {
    /// A value read from a documented sound layout.
    Known(T),
    /// Not known, with the reason recorded.
    Unknown {
        /// Why this field is not known, quoted from the family inventory.
        reason: &'static str,
    },
}

impl<T> SoundField<T> {
    /// Whether this stage knows the value.
    pub const fn is_known(&self) -> bool {
        matches!(self, Self::Known(_))
    }

    /// The value, when it is known.
    pub const fn known(&self) -> Option<&T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown { .. } => None,
        }
    }

    /// Why this field is not known, when it is not.
    pub const fn reason(&self) -> Option<&'static str> {
        match self {
            Self::Known(_) => None,
            Self::Unknown { reason } => Some(reason),
        }
    }
}

/// The declared sample descriptor of one sound entry.
///
/// Every field is reported, never invented: a sound entry whose header layout is
/// documented later keeps its span and its raw bytes here and gains its format,
/// channels, rate and loop metadata from that layout, not from a default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundDescriptor {
    format: SoundField<&'static str>,
    channels: SoundField<u8>,
    rate_hz: SoundField<u32>,
    loop_points: SoundField<(u32, u32)>,
}

impl SoundDescriptor {
    /// A descriptor whose every field carries `reason`.
    pub const fn undeclared(reason: &'static str) -> Self {
        Self {
            format: SoundField::Unknown { reason },
            channels: SoundField::Unknown { reason },
            rate_hz: SoundField::Unknown { reason },
            loop_points: SoundField::Unknown { reason },
        }
    }

    /// The entry's sample format, when it is known.
    pub const fn format(&self) -> SoundField<&'static str> {
        self.format
    }

    /// The entry's channel count, when it is known.
    pub const fn channels(&self) -> SoundField<u8> {
        self.channels
    }

    /// The entry's sample rate in hertz, when it is known.
    pub const fn rate_hz(&self) -> SoundField<u32> {
        self.rate_hz
    }

    /// The entry's loop points (start, end) in samples, when they are known.
    pub const fn loop_points(&self) -> SoundField<(u32, u32)> {
        self.loop_points
    }
}

/// One sound entry: its identity, its source span, its verbatim bytes and its
/// declared descriptor.
///
/// `content` borrows the container bytes; `name` stays raw bytes and `id`
/// stays verbatim, duplicates included (spec F06 non-negotiable #3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundEntry<'a> {
    index: usize,
    name: &'a [u8],
    id: Option<u32>,
    span: SourceSpan,
    content: &'a [u8],
    descriptor: SoundDescriptor,
}

impl<'a> SoundEntry<'a> {
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

    /// The entry's declared sample descriptor.
    pub const fn descriptor(&self) -> SoundDescriptor {
        self.descriptor
    }
}

/// A sound container: the bounded listing plus this stage's sound entries.
///
/// Every entry keeps its span and its bytes; none of them is decodable, so the
/// archive is an inventory of the container's members and never a claim that
/// anything can be played.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundArchive<'a> {
    listing: ArchiveListing<'a>,
    origin: FamilyOrigin,
    header_status: HeaderStatus,
}

impl<'a> SoundArchive<'a> {
    /// Provenance label of the archive.
    pub fn container(&self) -> &str {
        self.listing.container()
    }

    /// The family this archive is: always [`ZbdFamily::Sound`], because the gate
    /// in [`read_sound_archive`] refuses anything else.
    pub const fn family(&self) -> ZbdFamily {
        ZbdFamily::Sound
    }

    /// How the container's family was decided.
    pub const fn origin(&self) -> FamilyOrigin {
        self.origin
    }

    /// What was established about the container's header bytes.
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
    pub fn entry(&self, index: usize) -> Option<SoundEntry<'a>> {
        let row = self.listing.row(index)?;
        let content = self.listing.member_bytes(index)?;
        Some(SoundEntry {
            index: row.index(),
            name: row.name(),
            id: row.id(),
            span: row.span(),
            content,
            descriptor: SoundDescriptor::undeclared(undocumented_reason(ZbdFamily::Sound)),
        })
    }

    /// Every readable entry, in declared order.
    pub fn entries(&self) -> impl Iterator<Item = SoundEntry<'a>> + '_ {
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

    /// Every entry this stage cannot decode, with its span.
    ///
    /// Today that is every readable entry, because no sound header layout is
    /// documented; the list shrinks as evidence lands.
    pub fn unsupported_records(&self) -> Vec<UnsupportedRecord<'a>> {
        self.listing
            .rows()
            .iter()
            .filter(|row| row.is_readable())
            .map(|row| UnsupportedRecord::new(row, undocumented_reason(ZbdFamily::Sound)))
            .collect()
    }
}

/// Why a sound archive could not be produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoundError {
    /// The container carries another family's data; the sound reader refuses it
    /// instead of reading it with a parser that does not fit (spec F06 AC02).
    Family(FamilyMismatch),
    /// The listing itself could not be produced.
    Container(ContainerError),
}

impl SoundError {
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

impl From<FamilyMismatch> for SoundError {
    fn from(error: FamilyMismatch) -> Self {
        Self::Family(error)
    }
}

impl From<ContainerError> for SoundError {
    fn from(error: ContainerError) -> Self {
        Self::Container(error)
    }
}

impl fmt::Display for SoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Family(mismatch) => write!(f, "{mismatch}"),
            Self::Container(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SoundError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Family(mismatch) => Some(mismatch),
            Self::Container(error) => Some(error),
        }
    }
}

/// Reads the sound container `table` declares inside `bytes`.
///
/// `bytes` is the whole container; `table` is its member index (see
/// [`MemberTable`] for why the index is an input in this stage). A sound table
/// comes from a dispatch of a `ZBD/sounds*.zbd` archive (task #340); a table
/// built from the dispatch of another family, such as a reader or interp
/// container, is refused here with [`SoundError::Family`], exactly as the
/// reader reader refuses it.
///
/// # Errors
///
/// [`SoundError::Family`] when `table` is not a [`ZbdFamily::Sound`] index, and
/// [`SoundError::Container`] when the member table does not fit the parse's
/// allocation budget.
pub fn read_sound_archive<'a>(
    context: &mut ParseContext,
    table: &'a MemberTable<'a>,
    bytes: &'a [u8],
) -> Result<SoundArchive<'a>, SoundError> {
    require_family(table, ZbdFamily::Sound)?;
    let listing = list_members(context, bytes, table)?;
    Ok(SoundArchive {
        listing,
        origin: table.origin(),
        header_status: table.header_status(),
    })
}
