//! Decoding the `data` payload of one sound member (stage `### F06-C`, block
//! codecs in task #444).
//!
//! # What this stage decodes, and what it does not
//!
//! Task #344 established that the members of `ZBD/sounds*.zbd` are RIFF/WAVE
//! files and read each one's `fmt ` header
//! ([`crate::zbd::wave::read_wave_header`]). The header *declares* the sample
//! layout; this module turns the `data` payload into frames and per-channel
//! sample values **according to that declaration** and nothing else. The
//! stage's minimum acceptance case (spec F06 AC03) is exactly that: decode a
//! short sound sample and compare the byte and sample counts with what its
//! declared format implies.
//!
//! Two entry points, kept separate because the stage that added a codec is a
//! different piece of work from the consumer that switches to it:
//!
//! * [`SampleFormat::from_header`] is stage F06-C's plan. It decodes
//!   uncompressed PCM and refuses a compressed member with
//!   [`SampleFormatError::UnsupportedFormat`] carrying its own declared tag.
//! * [`SampleFormat::from_member`] is task #444's plan. It decodes uncompressed
//!   PCM, IMA ADPCM ([`crate::zbd::adpcm`]) and MS ADPCM from the member's own
//!   bytes, including the `fmt ` extension those two tags carry.
//!
//! The two entry points still differ — [`SampleFormat::from_header`] never
//! decodes a block codec — but the runtime consumer in `cs_assets` no longer
//! calls it: since Rally task #524 that consumer plans each member through this
//! module's block-aware entry point
//! ([`SampleFormat::from_header_with_blocks`], which
//! [`SampleFormat::from_member`] wraps), so a retail ADPCM member is decoded
//! rather than reported `UnsupportedFormat`. The block geometry and codebook
//! live in [`crate::zbd::adpcm`], and every value they need that varies per
//! member is read from the member.
//!
//! A compressed member is never approximated, never silently passed through as
//! if it were PCM and never refused for want of a value this crate could have
//! read: a plan is either the member's own declaration or an error naming it.
//!
//! # The declaration is the member's own, never this crate's
//!
//! [`SampleFormat::from_descriptor`] builds a decode plan out of a
//! [`SoundDescriptor`], whose every value came from the member's own WAVE
//! header. A field the header did not declare stays
//! [`SoundField::Unknown`](crate::zbd::SoundField::Unknown) and the plan
//! refuses with the header's own recorded reason: this module never fills in
//! a channel count, a rate or a format tag of its own. The PCM sample widths
//! it decodes (8-bit unsigned, 16- and 32-bit signed little-endian) are the
//! conventional RIFF/WAVE PCM layouts, not a claim about a specific archive.
//!
//! # Bounded, and honest about it
//!
//! Everything runs through [`ParseContext::parse`] (spec F03): the sample
//! buffer is booked against the parse's allocation budget **before** it
//! exists, so a hostile `data` length costs a refusal and not an allocation,
//! and a refused attempt leaves the ledger as it found it so the same member
//! can be retried with a bigger budget. For a compressed member the booked
//! count comes from the block geometry alone ([`AdpcmLayout::sample_count`]),
//! and a checked build compares the decode against that count, so a geometry
//! that disagreed with the decode would fail there rather than pass silently.
//! Nothing is resampled, filtered, normalised or mixed: the values are the
//! decoded samples, widened to `i32`, and what an audio consumer does with them
//! is F41's work.

use std::fmt;
use std::ops::Range;

use cs_types::evidence::SourceSpan;

use super::adpcm::{AdpcmError, AdpcmExtension, AdpcmLayout, read_adpcm_extension};
use super::sound_archive::{SoundDescriptor, SoundField};
use super::wave::{WAVE_FORMAT_PCM, WaveError, WaveHeader, read_wave_header};
use crate::error::ParseError;
use crate::io::ParseContext;

/// Error scope stamped onto failures raised inside this module.
pub const SAMPLE_ENTRYPOINT: &str = "zbd.sample";

/// Bytes one decoded sample value occupies, whatever the member's width.
pub const SAMPLE_VALUE_BYTES: u64 = size_of::<i32>() as u64;

/// One uncompressed PCM layout a member's WAVE header can declare.
///
/// The widths are the conventional RIFF/WAVE PCM sample sizes
/// (`wBitsPerSample` of 8, 16 or 32), not a property of any particular
/// archive. A member declaring anything else — every ADPCM tag, or a width
/// this module does not decode — is refused by [`SampleFormat::from_header`]
/// with its own declared values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PcmLayout {
    /// Unsigned 8-bit, as stored (the RIFF/WAVE convention; silence sits at
    /// 128, and this module does not shift it).
    Unsigned8,
    /// Signed 16-bit little-endian.
    Signed16Le,
    /// Signed 32-bit little-endian.
    Signed32Le,
}

impl PcmLayout {
    /// The `wBitsPerSample` this layout decodes.
    pub const fn bits_per_sample(self) -> u16 {
        match self {
            Self::Unsigned8 => 8,
            Self::Signed16Le => 16,
            Self::Signed32Le => 32,
        }
    }

    /// Stored bytes per sample of one channel.
    pub const fn bytes_per_sample(self) -> u64 {
        (self.bits_per_sample() / 8) as u64
    }

    /// The layout for a declared `wBitsPerSample`, when this module decodes
    /// it.
    pub const fn from_bits_per_sample(bits: u16) -> Option<Self> {
        match bits {
            8 => Some(Self::Unsigned8),
            16 => Some(Self::Signed16Le),
            32 => Some(Self::Signed32Le),
            _ => None,
        }
    }
}

impl fmt::Display for PcmLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pcm_s{}le", self.bits_per_sample())
    }
}

/// The sample layout one member's own WAVE header declares.
///
/// The variant is the format the member declares, never a preference of this
/// crate: `Pcm` for `wFormatTag` `0x0001`, and one block-coded variant per ADPCM
/// tag (`0x0011` IMA, `0x0002` Microsoft), each carrying the values the
/// member's `fmt ` extension declared ([`crate::zbd::adpcm`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleLayout {
    /// Uncompressed PCM at a documented width.
    Pcm(PcmLayout),
    /// A block-coded ADPCM layout and the values its member declared.
    Adpcm(AdpcmLayout),
}

impl SampleLayout {
    /// The uncompressed layout, when the member declares one.
    pub const fn pcm(self) -> Option<PcmLayout> {
        match self {
            Self::Pcm(layout) => Some(layout),
            Self::Adpcm(_) => None,
        }
    }

    /// The block-coded layout, when the member declares one.
    pub const fn adpcm(self) -> Option<AdpcmLayout> {
        match self {
            Self::Adpcm(layout) => Some(layout),
            Self::Pcm(_) => None,
        }
    }

    /// Whether the member's samples are block coded rather than stored one by
    /// one.
    pub const fn is_block_coded(self) -> bool {
        matches!(self, Self::Adpcm(_))
    }

    /// The `wBitsPerSample` the member declares.
    pub const fn bits_per_sample(self) -> u16 {
        match self {
            Self::Pcm(layout) => layout.bits_per_sample(),
            // Both ADPCM block layouts store four bits per sample.
            Self::Adpcm(_) => 4,
        }
    }

    /// Stored bytes per sample of one channel, for the layouts that store whole
    /// samples. A block-coded layout has none: a nibble is half a byte and the
    /// block is the unit, so this is `None` rather than a rounded-up fiction.
    pub const fn stored_bytes_per_sample(self) -> Option<u64> {
        match self {
            Self::Pcm(layout) => Some(layout.bytes_per_sample()),
            Self::Adpcm(_) => None,
        }
    }
}

impl fmt::Display for SampleLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pcm(layout) => write!(f, "{layout}"),
            Self::Adpcm(layout) => write!(f, "{layout}"),
        }
    }
}

/// Why a member's declared format could not become a decode plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFormatError {
    /// The member's WAVE header did not read, so nothing about its samples is
    /// known. `reason` is [`WaveError::reason`], the header's own explanation.
    UnreadableHeader {
        /// Why the header did not read.
        reason: &'static str,
    },
    /// The header declares a format tag this module does not decode.
    ///
    /// Carries the tag the member itself declared, so a diagnostic can say
    /// what the member is rather than only what is missing.
    UnsupportedFormat {
        /// The `wFormatTag` the member declares.
        tag: u16,
        /// The name RFC 2361 gives the tag, when it gives one.
        name: Option<&'static str>,
    },
    /// The header declares an ADPCM tag but its `fmt ` payload is shorter than
    /// the fields that tag documents, so no block layout can be read from it.
    ///
    /// Carries the member's own tag and the two lengths, and it is deliberately
    /// *not* [`Self::UnsupportedFormat`]: the tag is a codec this crate reads,
    /// and what is missing is the extension carrying its per-member values.
    AdpcmExtensionShort {
        /// The `wFormatTag` the member declares.
        tag: u16,
        /// Bytes the `fmt ` payload holds, the 16 common fields included.
        declared_len: u32,
        /// Bytes the tag's documented extension needs, the common fields
        /// included.
        needed: u32,
    },
    /// The member declares an MS ADPCM `wNumCoefs` above the seven pairs the
    /// layout reserves, so its coefficient table is one this module does not
    /// read.
    ///
    /// Carries the member's own tag and count. It is deliberately not
    /// [`Self::AdpcmExtensionShort`]: a payload can be long enough for the
    /// pairs the member declares, and a length shortfall that does not exist is
    /// a false diagnosis.
    AdpcmCoefficientTableTooLong {
        /// The `wFormatTag` the member declares.
        tag: u16,
        /// The `wNumCoefs` the member declares.
        declared: u16,
    },
    /// The header declares PCM at a `wBitsPerSample` this module does not
    /// decode.
    UnsupportedWidth {
        /// The declared `wBitsPerSample`.
        bits_per_sample: u16,
    },
    /// A field the header declared is missing, so the frame arithmetic cannot
    /// be done: no channel count, no rate, or a zero block align.
    ///
    /// `field` names the WAVE field (`nChannels`, `nSamplesPerSec`,
    /// `nBlockAlign`) and `reason` is the header's own recorded explanation
    /// for it being unknown.
    UndeclaredField {
        /// The WAVE field that is not known.
        field: &'static str,
        /// Why it is not known, quoted from the member's header.
        reason: &'static str,
    },
    /// `nBlockAlign` does not match what the declared channel count and
    /// sample width imply, so the header contradicts itself.
    BlockAlignMismatch {
        /// The declared `nBlockAlign`.
        declared: u16,
        /// What `nChannels * bytes_per_sample` comes to.
        implied: u32,
    },
    /// The declared `wSamplesPerBlock` does not match what the declared
    /// `nBlockAlign` and channel count hold, so the header contradicts itself.
    ///
    /// `implied` is the count one full block of `declared_block_align` bytes
    /// holds for one channel under the block layout the tag names.
    SamplesPerBlockMismatch {
        /// The declared `wSamplesPerBlock`.
        declared: u16,
        /// The declared `nBlockAlign`.
        declared_block_align: u16,
        /// What one full block of that size holds for one channel.
        implied: u64,
    },
    /// `nBlockAlign` cannot hold even one block header of the declared layout,
    /// so no sample of the member can be located.
    BlockAlignTooSmall {
        /// The declared `nBlockAlign`.
        declared: u16,
        /// Bytes the layout's block header needs for the declared channels.
        minimum: u64,
    },
    /// A full block of the declared `nBlockAlign` holds no whole per-channel
    /// group of nibble bytes, so the declaration contradicts itself before any
    /// block is decoded.
    ///
    /// This is the same condition as [`AdpcmError::PartialBlock`], found while
    /// planning rather than while decoding, and it is reported as that error
    /// rather than as [`Self::BlockAlignTooSmall`] — the block is not too small
    /// for its header, it cannot be split into the channels it declares.
    Block(AdpcmError),
    /// The declared channel count is not one this block layout is read for: no
    /// retail member declares it, and the layouts documented for wider blocks
    /// differ, so it is refused rather than guessed at.
    AdpcmChannelsNotObserved {
        /// The RFC 2361 name of the layout the member declares.
        layout: &'static str,
        /// The declared channel count.
        channels: u16,
    },
    /// `nChannels` is zero, which is not a channel count.
    ZeroChannels,
}

impl SampleFormatError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnreadableHeader { .. } => "unreadable_header",
            Self::UnsupportedFormat { .. } => "unsupported_format",
            Self::AdpcmExtensionShort { .. } => "adpcm_extension_short",
            Self::AdpcmCoefficientTableTooLong { .. } => "adpcm_coefficient_table_too_long",
            Self::UnsupportedWidth { .. } => "unsupported_width",
            Self::UndeclaredField { .. } => "undeclared_field",
            Self::BlockAlignMismatch { .. } => "block_align_mismatch",
            Self::SamplesPerBlockMismatch { .. } => "samples_per_block_mismatch",
            Self::BlockAlignTooSmall { .. } => "block_align_too_small",
            Self::Block(error) => error.code(),
            Self::AdpcmChannelsNotObserved { .. } => "adpcm_channels_not_observed",
            Self::ZeroChannels => "zero_channels",
        }
    }

    /// The member's recorded explanation, when the failure is a field the
    /// header itself did not declare.
    pub const fn reason(&self) -> Option<&'static str> {
        match self {
            Self::UnreadableHeader { reason } | Self::UndeclaredField { reason, .. } => {
                Some(reason)
            }
            _ => None,
        }
    }
}

impl fmt::Display for SampleFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnreadableHeader { reason } => {
                write!(f, "the member's WAVE header did not read: {reason}")
            }
            Self::UnsupportedFormat { tag, name } => match name {
                Some(name) => write!(
                    f,
                    "the member declares {name} (`wFormatTag` 0x{tag:04X}); this entry point \
                     decodes uncompressed PCM only"
                ),
                None => write!(
                    f,
                    "the member declares an unnamed format (`wFormatTag` 0x{tag:04X}); this \
                     entry point decodes uncompressed PCM only"
                ),
            },
            Self::AdpcmExtensionShort {
                tag,
                declared_len,
                needed,
            } => write!(
                f,
                "the member declares an ADPCM format (`wFormatTag` 0x{tag:04X}) whose `fmt ` \
                 payload holds {declared_len} bytes; the fields that tag documents need {needed}"
            ),
            Self::UnsupportedWidth { bits_per_sample } => write!(
                f,
                "the member declares {bits_per_sample} bits per sample; uncompressed PCM is \
                 decoded at 8, 16 and 32 and the ADPCM block layouts at 4"
            ),
            Self::UndeclaredField { field, reason } => {
                write!(f, "the member's WAVE header declares no {field}: {reason}")
            }
            Self::BlockAlignMismatch { declared, implied } => write!(
                f,
                "the member declares `nBlockAlign` {declared}, but {implied} follows from its \
                 channels and sample width"
            ),
            Self::SamplesPerBlockMismatch {
                declared,
                declared_block_align,
                implied,
            } => write!(
                f,
                "the member declares `wSamplesPerBlock` {declared}, but a block of \
                 `nBlockAlign` {declared_block_align} holds {implied} sample(s) per channel"
            ),
            Self::BlockAlignTooSmall { declared, minimum } => write!(
                f,
                "the member declares `nBlockAlign` {declared}, too small for the {minimum}-byte \
                 block header its layout documents"
            ),
            Self::AdpcmCoefficientTableTooLong { tag, declared } => write!(
                f,
                "the member declares an ADPCM format (`wFormatTag` 0x{tag:04X}) with {declared} \
                 coefficient pairs; the layout this module reads reserves {}",
                super::adpcm::MS_COEFFICIENT_PAIRS
            ),
            Self::Block(error) => write!(f, "{error}"),
            Self::AdpcmChannelsNotObserved { layout, channels } => write!(
                f,
                "the member declares {layout} with {channels} channels; no retail member does, \
                 and the layouts documented for wider blocks differ"
            ),
            Self::ZeroChannels => f.write_str("the member declares no channels"),
        }
    }
}

impl std::error::Error for SampleFormatError {}

impl From<WaveError> for SampleFormatError {
    fn from(error: WaveError) -> Self {
        Self::UnreadableHeader {
            reason: error.reason(),
        }
    }
}

/// The reason a [`SoundField`] is unknown, carried by the refusals that quote
/// it.
fn unknown_reason<T: Copy>(field: &SoundField<T>) -> &'static str {
    field
        .reason()
        .expect("an unknown field always carries its reason")
}

/// The `fmt ` payload's byte range inside a member, as its header located it.
///
/// A header that did not read has no such span, and neither has one whose
/// offsets do not fit `usize`; both produce a range no slice has, so the caller's
/// `get` refuses instead of panicking.
fn fmt_range(header: &WaveHeader) -> Range<usize> {
    let span = header.fmt_span();
    let start = usize::try_from(span.offset).unwrap_or(usize::MAX);
    let length = usize::try_from(span.length).unwrap_or(0);
    start..start.saturating_add(length)
}

/// A decode plan built from what one member's WAVE header declares.
///
/// Every value here is the member's own, read by
/// [`crate::zbd::wave::read_wave_header`] and, for a block-coded member, from
/// the `fmt ` extension its own bytes carry; this type only checks that the
/// declaration is self-consistent and that it is a layout this crate decodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleFormat {
    layout: SampleLayout,
    channels: u16,
    rate_hz: u32,
    block_align: u16,
    data: SourceSpan,
}
impl SampleFormat {
    /// Builds stage F06-C's plan: what a member's read WAVE header declares,
    /// decoded as uncompressed PCM.
    ///
    /// # Errors
    ///
    /// [`SampleFormatError::UnsupportedFormat`] for a tag this entry point does
    /// not decode (carrying the tag and its RFC 2361 name),
    /// [`SampleFormatError::UnsupportedWidth`] for a PCM width this stage
    /// does not decode, [`SampleFormatError::UndeclaredField`] when a field
    /// the frame arithmetic needs is [`crate::zbd::SoundField::Unknown`]
    /// (quoting that field's own reason),
    /// [`SampleFormatError::BlockAlignMismatch`] when `nBlockAlign` contradicts
    /// the channel count and sample width, and
    /// [`SampleFormatError::ZeroChannels`] for a declared channel count of
    /// zero.
    pub fn from_header(header: &WaveHeader) -> Result<Self, SampleFormatError> {
        let descriptor = SoundDescriptor::from_wave(header);
        Self::from_descriptor(&descriptor, header.data_span())
    }

    /// Builds task #444's plan from a member's own bytes: what its WAVE header
    /// declares, including the block codecs.
    ///
    /// `member` is the member's whole bytes exactly as the container stores
    /// them. The header is read by
    /// [`crate::zbd::wave::read_wave_header`] and the `fmt ` extension out of
    /// the payload that header located, so every value in the plan comes from
    /// the member.
    ///
    /// # Errors
    ///
    /// As [`Self::from_header_with_blocks`].
    pub fn from_member(member: &[u8]) -> Result<Self, SampleFormatError> {
        let header = read_wave_header(member)?;
        let fmt = member
            .get(fmt_range(&header))
            .ok_or(SampleFormatError::UnreadableHeader {
                reason: "the member's `fmt ` payload lies outside the member",
            })?;
        Self::from_header_with_blocks(&header, fmt)
    }

    /// Builds task #444's plan from a member's read header and the `fmt ` payload
    /// its [`crate::zbd::wave::WaveHeader::fmt_span`] points at.
    ///
    /// Split from [`Self::from_member`] for a caller that already holds the
    /// header and the `fmt ` payload — a private research export, or a cache
    /// that stored them.
    ///
    /// # Errors
    ///
    /// The errors of [`Self::from_descriptor`] for the common fields, plus, for
    /// a member that declares an ADPCM tag:
    ///
    /// * [`SampleFormatError::AdpcmExtensionShort`] when its `fmt ` payload is
    ///   shorter than the fields that tag documents,
    /// * [`SampleFormatError::AdpcmCoefficientTableTooLong`] when a MS ADPCM
    ///   member declares more coefficient pairs than the layout reserves,
    /// * [`SampleFormatError::UnsupportedWidth`] for a `wBitsPerSample` that is
    ///   not 4, the only width the two block layouts store,
    /// * [`SampleFormatError::BlockAlignTooSmall`] when `nBlockAlign` cannot
    ///   hold one block header of the declared layout,
    /// * [`SampleFormatError::Block`] when a full block of that size holds no
    ///   whole per-channel group of nibble bytes, and
    /// * [`SampleFormatError::SamplesPerBlockMismatch`] when the declared
    ///   `wSamplesPerBlock` is not what a full block of the declared
    ///   `nBlockAlign` holds for one channel.
    pub fn from_header_with_blocks(
        header: &WaveHeader,
        fmt: &[u8],
    ) -> Result<Self, SampleFormatError> {
        let descriptor = SoundDescriptor::from_wave(header);
        let extension = read_adpcm_extension(header.format_tag(), fmt);
        Self::from_declared(&descriptor, extension, header.data_span())
    }

    /// Builds the plan a member's declared descriptor describes, with the
    /// `data` payload's span.
    ///
    /// Split from [`Self::from_header`] so a caller holding only a
    /// [`SoundDescriptor`] (for example one read from a cache) can still
    /// build the plan, and so the tests can exercise a descriptor that is not
    /// backed by a readable header.
    ///
    /// This is stage F06-C's plan and stays uncompressed: a block-coded
    /// descriptor needs the `fmt ` extension a [`SoundDescriptor`] does not
    /// carry, so it is refused here with
    /// [`SampleFormatError::UnsupportedFormat`]. Use
    /// [`Self::from_header_with_blocks`] for a member's own bytes.
    ///
    /// # Errors
    ///
    /// As [`Self::from_header`].
    pub fn from_descriptor(
        descriptor: &SoundDescriptor,
        data: SourceSpan,
    ) -> Result<Self, SampleFormatError> {
        // This entry point decodes uncompressed PCM only, exactly as stage
        // F06-C did: a block-coded tag is refused with the member's own tag,
        // because a `SoundDescriptor` does not carry the `fmt ` extension those
        // layouts need. Read the member's bytes and use
        // [`Self::from_header_with_blocks`] for a compressed member.
        match descriptor.format_tag().known() {
            Some(&tag) if tag != WAVE_FORMAT_PCM => {
                return Err(SampleFormatError::UnsupportedFormat {
                    tag,
                    name: descriptor.format().known().copied(),
                });
            }
            Some(_) => {}
            None => {
                return Err(SampleFormatError::UnreadableHeader {
                    reason: unknown_reason(&descriptor.format_tag()),
                });
            }
        }
        Self::from_declared(descriptor, AdpcmExtension::Absent, data)
    }

    /// The plan for a descriptor plus the `fmt ` extension read beside it.
    ///
    /// Both public constructors funnel through here, so the checks below are the
    /// only place a declaration is judged.
    fn from_declared(
        descriptor: &SoundDescriptor,
        extension: AdpcmExtension,
        data: SourceSpan,
    ) -> Result<Self, SampleFormatError> {
        // Each field is read by matching on its own `SoundField`, so an
        // unknown field's reason is only ever produced for a field that is
        // actually unknown.
        let Some(&tag) = descriptor.format_tag().known() else {
            return Err(SampleFormatError::UnreadableHeader {
                reason: unknown_reason(&descriptor.format_tag()),
            });
        };
        let name = descriptor.format().known().copied();
        let Some(&bits_per_sample) = descriptor.bits_per_sample().known() else {
            return Err(SampleFormatError::UndeclaredField {
                field: "wBitsPerSample",
                reason: unknown_reason(&descriptor.bits_per_sample()),
            });
        };
        let Some(&channels) = descriptor.channels().known() else {
            return Err(SampleFormatError::UndeclaredField {
                field: "nChannels",
                reason: unknown_reason(&descriptor.channels()),
            });
        };
        if channels == 0 {
            return Err(SampleFormatError::ZeroChannels);
        }
        let Some(&rate_hz) = descriptor.rate_hz().known() else {
            return Err(SampleFormatError::UndeclaredField {
                field: "nSamplesPerSec",
                reason: unknown_reason(&descriptor.rate_hz()),
            });
        };
        let Some(&block_align) = descriptor.block_align().known() else {
            return Err(SampleFormatError::UndeclaredField {
                field: "nBlockAlign",
                reason: unknown_reason(&descriptor.block_align()),
            });
        };
        let adpcm = match extension {
            AdpcmExtension::Ima { .. } | AdpcmExtension::Ms { .. } => extension.layout(),
            AdpcmExtension::Short {
                tag,
                declared_len,
                needed,
            } => {
                return Err(SampleFormatError::AdpcmExtensionShort {
                    tag,
                    declared_len,
                    needed,
                });
            }
            AdpcmExtension::TooManyCoefficients { tag, declared } => {
                return Err(SampleFormatError::AdpcmCoefficientTableTooLong { tag, declared });
            }
            // No extension was read for this tag: the plan is uncompressed, and
            // a tag this crate does not decode is refused with the member's own.
            AdpcmExtension::Absent => None,
        };
        let layout = if let Some(adpcm) = adpcm {
            if bits_per_sample != 4 {
                return Err(SampleFormatError::UnsupportedWidth { bits_per_sample });
            }
            if !adpcm.supports(channels) {
                return Err(SampleFormatError::AdpcmChannelsNotObserved {
                    layout: super::wave::format_name(adpcm.format_tag())
                        .unwrap_or("an unnamed format"),
                    channels,
                });
            }
            let minimum = adpcm.block_header_bytes(channels);
            if u64::from(block_align) < minimum {
                return Err(SampleFormatError::BlockAlignTooSmall {
                    declared: block_align,
                    minimum,
                });
            }
            let declared = adpcm.declared_samples_per_block();
            // A full block of the declared size has to hold a whole number of
            // per-channel groups, and the refusal says which of the two ways it
            // can fail it was: too small for the header, or not divisible into
            // the declared channels.
            let implied = match adpcm.full_block_sample_count(channels, u64::from(block_align)) {
                Ok(implied) => implied,
                Err(AdpcmError::ShortBlock { .. }) => {
                    return Err(SampleFormatError::BlockAlignTooSmall {
                        declared: block_align,
                        minimum,
                    });
                }
                Err(error) => return Err(SampleFormatError::Block(error)),
            };
            if u64::from(declared) != implied {
                return Err(SampleFormatError::SamplesPerBlockMismatch {
                    declared,
                    declared_block_align: block_align,
                    implied,
                });
            }
            SampleLayout::Adpcm(adpcm)
        } else {
            if tag != WAVE_FORMAT_PCM {
                return Err(SampleFormatError::UnsupportedFormat { tag, name });
            }
            let Some(pcm) = PcmLayout::from_bits_per_sample(bits_per_sample) else {
                return Err(SampleFormatError::UnsupportedWidth { bits_per_sample });
            };
            let implied =
                u32::from(channels) * u32::try_from(pcm.bytes_per_sample()).unwrap_or(u32::MAX);
            if u32::from(block_align) != implied {
                return Err(SampleFormatError::BlockAlignMismatch {
                    declared: block_align,
                    implied,
                });
            }
            SampleLayout::Pcm(pcm)
        };
        Ok(Self {
            layout,
            channels,
            rate_hz,
            block_align,
            data,
        })
    }

    /// The layout the member declares: uncompressed PCM or a block codec.
    pub const fn layout(&self) -> SampleLayout {
        self.layout
    }

    /// The channel count the member declares.
    pub const fn channels(&self) -> u16 {
        self.channels
    }

    /// The sample rate the member declares, in hertz.
    pub const fn rate_hz(&self) -> u32 {
        self.rate_hz
    }

    /// The `nBlockAlign` the member declares, in bytes.
    pub const fn block_align(&self) -> u16 {
        self.block_align
    }

    /// The `data` payload inside the member this plan describes.
    pub const fn data_span(&self) -> SourceSpan {
        self.data
    }

    /// Stored bytes one frame of this format occupies: `nBlockAlign`.
    ///
    /// For a block-coded member a frame is a whole block, which the
    /// constructor has checked against the block layout's geometry; for PCM it
    /// is one sample per channel, which it has checked against the sample
    /// width.
    pub const fn frame_bytes(&self) -> u64 {
        self.block_align as u64
    }

    /// How many sample values one frame holds in total: one per channel for
    /// PCM, and the block's `wSamplesPerBlock` for each of its channels for a
    /// block-coded member.
    pub const fn samples_per_frame(&self) -> u64 {
        match self.layout {
            SampleLayout::Pcm(_) => self.channels as u64,
            SampleLayout::Adpcm(adpcm) => {
                adpcm.declared_samples_per_block() as u64 * self.channels as u64
            }
        }
    }
}

/// One member's `data` payload, decoded.
///
/// `frames` counts declared frames, which is what `nBlockAlign` names: one
/// sample per channel for PCM, and one block for a block-coded member. A
/// block-coded member's last block may be shorter than `nBlockAlign` — that is
/// the format's own final block, not a contradiction — and it is counted as a
/// frame like any other, which is why `byte_len` is the payload's own length
/// rather than `frames * frame_bytes` there. `samples` counts the values in
/// them, one per channel and frame, widened to `i32` and otherwise exactly as
/// the format produces them. `byte_len` is the payload the decode accounted
/// for: `frames * frame_bytes` for a PCM payload, which must be a whole number
/// of frames ([`SampleError::PartialFrame`]), and the payload's own length for a
/// block-coded member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedSound {
    format: SampleFormat,
    frames: u64,
    byte_len: u64,
    samples: Vec<i32>,
}

impl DecodedSound {
    /// The declaration the samples were decoded under.
    pub const fn format(&self) -> &SampleFormat {
        &self.format
    }

    /// How many declared frames the payload holds: samples per channel for
    /// PCM, blocks for a block-coded member, a trailing short block included.
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    /// How many sample values one frame holds in total: one per channel for
    /// PCM, and the block's `wSamplesPerBlock` for each of its channels for a
    /// block-coded member.
    pub const fn samples_per_frame(&self) -> u64 {
        self.format.samples_per_frame()
    }

    /// How many sample values the payload holds in total.
    pub fn sample_count(&self) -> u64 {
        self.samples.len() as u64
    }

    /// How many bytes of `data` the decode accounted for.
    pub const fn byte_len(&self) -> u64 {
        self.byte_len
    }

    /// The decoded values, frame by frame and channel by channel, widened to
    /// `i32` and otherwise exactly as the declared layout produces them.
    pub fn samples(&self) -> &[i32] {
        &self.samples
    }

    /// The values of one frame, in the order the frame stores them, or `None`
    /// when the frame is past the end of the decode.
    ///
    /// A PCM frame holds one value per channel, in channel order. A block holds
    /// `wSamplesPerBlock` values for each of its channels, **frame by frame and
    /// channel by channel**, which is the order the block's own data is read in:
    /// the two history values of every channel (the older one first) are its
    /// first two frames, and each following byte is one frame.
    ///
    /// `None` therefore also covers an index whose frame the payload does not
    /// hold in full: a block-coded member's trailing short block is counted by
    /// [`Self::frames`] but holds fewer values than a full one.
    pub fn frame(&self, index: u64) -> Option<&[i32]> {
        let per_frame = self.samples_per_frame();
        if per_frame == 0 {
            return None;
        }
        let start = usize::try_from(index.checked_mul(per_frame)?).ok()?;
        let end = start.checked_add(usize::try_from(per_frame).ok()?)?;
        self.samples.get(start..end)
    }
}

/// Why a member's samples could not be decoded.
///
/// Every variant carries the member's declared values and counts — never
/// sample bytes (F03: errors carry metadata only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SampleError {
    /// The member's declared format is not one this stage decodes, or the
    /// declaration is not self-consistent.
    Format(SampleFormatError),
    /// The `data` payload is not a whole number of declared frames, so the
    /// header and the payload disagree about its length.
    PartialFrame {
        /// Bytes the `data` payload holds.
        bytes: u64,
        /// Bytes one declared frame occupies.
        frame_bytes: u64,
        /// The bytes left over after the whole frames.
        remainder: u64,
    },
    /// A block of a block-coded member could not be read: too short for its own
    /// header, not a whole number of per-channel groups, or naming a step index
    /// or coefficient the member's own declaration does not hold.
    Block(AdpcmError),
    /// A failure of the parse itself (the allocation budget, or the entrypoint
    /// scope), already stamped `zbd.sample` by [`ParseContext::parse`].
    Parse(ParseError),
}

impl SampleError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Format(error) => error.code(),
            Self::PartialFrame { .. } => "partial_frame",
            Self::Block(error) => error.code(),
            Self::Parse(error) => match error.kind {
                crate::ParseErrorKind::AllocationBudgetExceeded => "allocation_budget_exceeded",
                crate::ParseErrorKind::LengthOverflow => "length_overflow",
                _ => "parse",
            },
        }
    }

    /// The member's recorded explanation, when the failure is one the header
    /// itself explains.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Self::Format(error) => error.reason(),
            _ => None,
        }
    }

    /// The byte offset the failure is anchored at.
    ///
    /// A [`Self::Parse`] failure is anchored inside the member, and so is
    /// [`Self::PartialFrame`]. A [`Self::Block`] failure carries the offset the
    /// block decoder found, which counts from the start of the `data` payload:
    /// add the plan's [`crate::zbd::SourceSpan::offset`] to place it in the
    /// member.
    pub fn offset(&self) -> u64 {
        match self {
            Self::Format(_) => 0,
            Self::PartialFrame { bytes, .. } => *bytes,
            Self::Block(error) => error.offset(),
            Self::Parse(error) => error.offset,
        }
    }
}

impl fmt::Display for SampleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(error) => write!(f, "{error}"),
            Self::PartialFrame {
                bytes,
                frame_bytes,
                remainder,
            } => write!(
                f,
                "the member's `data` payload holds {bytes} bytes, which is not a whole number of \
                 {frame_bytes}-byte declared frames: {remainder} trailing byte(s)"
            ),
            Self::Block(error) => write!(f, "{error}"),
            Self::Parse(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SampleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Format(error) => Some(error),
            Self::Block(error) => Some(error),
            Self::Parse(error) => Some(error),
            Self::PartialFrame { .. } => None,
        }
    }
}

impl From<SampleFormatError> for SampleError {
    fn from(error: SampleFormatError) -> Self {
        Self::Format(error)
    }
}

impl From<ParseError> for SampleError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl SampleError {
    /// A member whose WAVE header did not read: the decode never starts, and
    /// the failure carries the header reader's own explanation.
    pub fn from_wave_header(error: WaveError) -> Self {
        Self::Format(SampleFormatError::from(error))
    }
}

/// Decodes `member` under the plan its own WAVE header declares.
///
/// `format` is the member's own [`SampleFormat`] — built by
/// [`SampleFormat::from_header`] or, for a block-coded member, by
/// [`SampleFormat::from_member`] — and `member` is that member's whole bytes.
/// The decode reads the `data` payload the plan points at and nothing else.
///
/// This is the stage's minimum acceptance case (spec F06 AC03): the result's
/// `byte_len` is the payload's length, its `sample_count` is
/// `frames * samples_per_frame`, and for PCM the two satisfy
/// `byte_len == sample_count * bytes_per_sample` because the constructor
/// checked `nBlockAlign` against the channel count and the sample width. For a
/// block-coded member the count comes from the block geometry, which the
/// constructor checked against the declared `wSamplesPerBlock`.
///
/// # Errors
///
/// [`SampleError::PartialFrame`] when a PCM member's `data` payload is not a
/// whole number of declared frames, [`SampleError::Block`] when a block cannot be
/// read, and [`SampleError::Parse`] when the sample buffer does not fit the
/// parse's allocation budget.
pub fn decode_sound_sample(
    context: &mut ParseContext,
    member: &[u8],
    format: &SampleFormat,
) -> Result<DecodedSound, SampleError> {
    let data = format.data_span();
    let offset = data.offset;
    let span_error = |what: &str, observed: String| {
        SampleError::Parse(ParseError::length_overflow(
            context.container().to_owned(),
            offset,
            "sample.data",
            what.to_owned(),
            observed,
        ))
    };
    let start = usize::try_from(offset).map_err(|_| {
        span_error(
            "the `data` offset to fit in usize",
            format!("offset {offset}"),
        )
    })?;
    let length = usize::try_from(data.length).map_err(|_| {
        span_error(
            "the `data` length to fit in usize",
            format!("{} bytes", data.length),
        )
    })?;
    let end = start.checked_add(length).ok_or_else(|| {
        span_error(
            "the `data` payload to fit in usize",
            format!("offset {offset} plus {} bytes", data.length),
        )
    })?;
    let payload = member.get(start..end).ok_or_else(|| {
        span_error(
            "the `data` payload to lie inside the member",
            format!("{start}..{end} of {} bytes", member.len()),
        )
    })?;
    decode_payload(context, payload, format)
}

/// The decode itself, over the `data` payload.
///
/// Split out so a caller that already holds the payload — a test fixture, or
/// a member extracted by a private research export — can decode it without
/// re-slicing, and so both paths share one budget and one set of refusals.
pub fn decode_payload(
    context: &mut ParseContext,
    payload: &[u8],
    format: &SampleFormat,
) -> Result<DecodedSound, SampleError> {
    let frame_bytes = format.frame_bytes();
    let byte_len = payload.len() as u64;
    // A block-coded member's unit is a block, and its trailing block may be
    // shorter than `nBlockAlign`: that is the format's own final block, counted
    // from the block geometry, not a contradiction to refuse. Uncompressed PCM
    // has no such notion, so a partial frame is refused there.
    let (frames, sample_count) = match format.layout() {
        SampleLayout::Pcm(_) => {
            let remainder = byte_len % frame_bytes;
            if remainder != 0 {
                return Err(SampleError::PartialFrame {
                    bytes: byte_len,
                    frame_bytes,
                    remainder,
                });
            }
            let frames = byte_len / frame_bytes;
            let count = frames * format.samples_per_frame();
            (frames, count)
        }
        SampleLayout::Adpcm(adpcm) => {
            // Counted from the geometry alone, before anything is allocated, so
            // a hostile `data` length costs a refusal and not an allocation.
            let count = adpcm
                .sample_count(format.channels(), frame_bytes, byte_len)
                .map_err(SampleError::Block)?;
            let blocks = byte_len.div_ceil(frame_bytes);
            (blocks, count)
        }
    };
    let container = context.container().to_owned();
    let offset = format.data_span().offset;
    // A block that cannot be read is a typed [`AdpcmError`], not a parse
    // failure, so it is carried out of the attempt and reported as itself. The
    // attempt still returns an error, which is what rolls the reservation back:
    // a corrupt member must not leave a charge behind for its siblings.
    let mut block_error: Option<AdpcmError> = None;

    let decoded = context.parse(
        SAMPLE_ENTRYPOINT,
        payload,
        |_reader, allocation, _recursion| {
            // Booked before the buffer exists, so a hostile payload costs a
            // refusal and not an allocation.
            allocation.reserve("sample.values", offset, sample_count, SAMPLE_VALUE_BYTES)?;
            let capacity = usize::try_from(sample_count).map_err(|_| {
                ParseError::length_overflow(
                    container.clone(),
                    offset,
                    "sample.values",
                    "sample count to fit in usize".to_owned(),
                    format!("{sample_count} samples"),
                )
            })?;
            let mut samples: Vec<i32> = Vec::with_capacity(capacity);
            match format.layout() {
                SampleLayout::Pcm(layout) => {
                    decode_pcm_payload(layout, payload, frame_bytes, &mut samples);
                }
                SampleLayout::Adpcm(adpcm) => {
                    if let Err(error) =
                        adpcm.decode(format.channels(), frame_bytes, payload, &mut samples)
                    {
                        block_error = Some(error);
                        return Err(ParseError::length_overflow(
                            container.clone(),
                            offset + error.offset(),
                            "sample.values",
                            "the block to decode".to_owned(),
                            error.to_string(),
                        ));
                    }
                }
            }
            debug_assert_eq!(
                samples.len(),
                capacity,
                "one booked value per decoded sample"
            );
            debug_assert_eq!(samples.len() as u64, sample_count);
            Ok(DecodedSound {
                format: *format,
                frames,
                byte_len,
                samples,
            })
        },
    );
    match (decoded, block_error) {
        (Ok(decoded), _) => Ok(decoded),
        // A block error was already recorded; the parse error that rolled the
        // reservation back is the mechanism, not the message.
        (Err(_), Some(error)) => Err(SampleError::Block(error)),
        (Err(error), None) => Err(SampleError::Parse(error)),
    }
}

/// Reads a whole number of uncompressed PCM frames into `out`.
///
/// The plan has already checked that the payload is a whole number of frames and
/// that a frame is `nChannels` samples of the declared width, so this only reads
/// the stored bytes.
fn decode_pcm_payload(layout: PcmLayout, payload: &[u8], frame_bytes: u64, out: &mut Vec<i32>) {
    let sample_bytes = usize::try_from(layout.bytes_per_sample()).unwrap_or(usize::MAX);
    // A frame is `nBlockAlign` bytes: `nChannels` interleaved samples, so a
    // multi-channel frame yields `nChannels` values, not one.
    for frame in payload.chunks_exact(usize::try_from(frame_bytes).unwrap_or(usize::MAX)) {
        for sample in frame.chunks_exact(sample_bytes) {
            match layout {
                PcmLayout::Unsigned8 => out.push(i32::from(sample[0])),
                PcmLayout::Signed16Le => {
                    out.push(i32::from(i16::from_le_bytes([sample[0], sample[1]])))
                }
                PcmLayout::Signed32Le => out.push(i32::from_le_bytes([
                    sample[0], sample[1], sample[2], sample[3],
                ])),
            }
        }
    }
}
