//! Decoding the `data` payload of one sound member (stage `### F06-C`).
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
//! **Only uncompressed PCM is decoded.** Every retail member task #344
//! measured is IMA ADPCM, MS ADPCM or 8-bit PCM, and a compressed member is
//! reported as [`SampleError::UnsupportedFormat`] carrying its own declared
//! tag — never approximated and never silently passed through. Block-decoding
//! ADPCM is a separate, checked piece of work, not something to improvise
//! here.
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
//! can be retried with a bigger budget. Nothing is resampled, filtered,
//! normalised or mixed: the values are the stored samples, widened to `i32`,
//! and what an audio consumer does with them is F41's work.

use std::fmt;

use cs_types::evidence::SourceSpan;

use super::sound_archive::SoundDescriptor;
use super::wave::{WAVE_FORMAT_PCM, WaveError, WaveHeader};
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
    /// `nChannels` is zero, which is not a channel count.
    ZeroChannels,
}

impl SampleFormatError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnreadableHeader { .. } => "unreadable_header",
            Self::UnsupportedFormat { .. } => "unsupported_format",
            Self::UnsupportedWidth { .. } => "unsupported_width",
            Self::UndeclaredField { .. } => "undeclared_field",
            Self::BlockAlignMismatch { .. } => "block_align_mismatch",
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
                    "the member declares {name} (`wFormatTag` 0x{tag:04X}); this stage decodes \
                     uncompressed PCM only"
                ),
                None => write!(
                    f,
                    "the member declares an unnamed format (`wFormatTag` 0x{tag:04X}); this \
                     stage decodes uncompressed PCM only"
                ),
            },
            Self::UnsupportedWidth { bits_per_sample } => write!(
                f,
                "the member declares PCM at {bits_per_sample} bits per sample; this stage decodes \
                 8, 16 and 32"
            ),
            Self::UndeclaredField { field, reason } => {
                write!(f, "the member's WAVE header declares no {field}: {reason}")
            }
            Self::BlockAlignMismatch { declared, implied } => write!(
                f,
                "the member declares `nBlockAlign` {declared}, but {implied} follows from its \
                 channels and sample width"
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

/// A decode plan built from what one member's WAVE header declares.
///
/// Every value here is the member's own, read by
/// [`crate::zbd::wave::read_wave_header`]; this type only checks that the
/// declaration is self-consistent and that it is a layout this stage decodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleFormat {
    layout: PcmLayout,
    channels: u16,
    rate_hz: u32,
    block_align: u16,
    data: SourceSpan,
}

impl SampleFormat {
    /// Builds the plan a member's read WAVE header declares.
    ///
    /// # Errors
    ///
    /// [`SampleFormatError::UnsupportedFormat`] for a tag this stage does not
    /// decode (carrying the tag and its RFC 2361 name),
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

    /// Builds the plan a member's declared descriptor describes, with the
    /// `data` payload's span.
    ///
    /// Split from [`Self::from_header`] so a caller holding only a
    /// [`SoundDescriptor`] (for example one read from a cache) can still
    /// build the plan, and so the tests can exercise a descriptor that is not
    /// backed by a readable header.
    ///
    /// # Errors
    ///
    /// As [`Self::from_header`].
    pub fn from_descriptor(
        descriptor: &SoundDescriptor,
        data: SourceSpan,
    ) -> Result<Self, SampleFormatError> {
        // The tag is read first and by value: a member whose header did not
        // read has no tag at all, and is reported as such rather than as an
        // unsupported format.
        let Some(&tag) = descriptor.format_tag().known() else {
            return Err(SampleFormatError::UnreadableHeader {
                reason: descriptor
                    .format_tag()
                    .reason()
                    .expect("an unknown field always carries its reason"),
            });
        };
        if tag != WAVE_FORMAT_PCM {
            return Err(SampleFormatError::UnsupportedFormat {
                tag,
                name: descriptor.format().known().copied(),
            });
        }
        let bits_per_sample =
            *descriptor
                .bits_per_sample()
                .known()
                .ok_or(SampleFormatError::UndeclaredField {
                    field: "wBitsPerSample",
                    reason: descriptor
                        .bits_per_sample()
                        .reason()
                        .expect("an unknown field always carries its reason"),
                })?;
        let layout = PcmLayout::from_bits_per_sample(bits_per_sample)
            .ok_or(SampleFormatError::UnsupportedWidth { bits_per_sample })?;
        let channels =
            *descriptor
                .channels()
                .known()
                .ok_or(SampleFormatError::UndeclaredField {
                    field: "nChannels",
                    reason: descriptor
                        .channels()
                        .reason()
                        .expect("an unknown field always carries its reason"),
                })?;
        if channels == 0 {
            return Err(SampleFormatError::ZeroChannels);
        }
        let rate_hz = *descriptor
            .rate_hz()
            .known()
            .ok_or(SampleFormatError::UndeclaredField {
                field: "nSamplesPerSec",
                reason: descriptor
                    .rate_hz()
                    .reason()
                    .expect("an unknown field always carries its reason"),
            })?;
        let block_align =
            *descriptor
                .block_align()
                .known()
                .ok_or(SampleFormatError::UndeclaredField {
                    field: "nBlockAlign",
                    reason: descriptor
                        .block_align()
                        .reason()
                        .expect("an unknown field always carries its reason"),
                })?;
        let implied =
            u32::from(channels) * u32::try_from(layout.bytes_per_sample()).unwrap_or(u32::MAX);
        if u32::from(block_align) != implied {
            return Err(SampleFormatError::BlockAlignMismatch {
                declared: block_align,
                implied,
            });
        }
        Ok(Self {
            layout,
            channels,
            rate_hz,
            block_align,
            data,
        })
    }

    /// The PCM layout the member declares.
    pub const fn layout(&self) -> PcmLayout {
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

    /// Stored bytes one frame of this format occupies:
    /// `nBlockAlign`, which the constructor has checked against the channel
    /// count and the sample width.
    pub const fn frame_bytes(&self) -> u64 {
        self.block_align as u64
    }

    /// How many sample values one frame holds: one per declared channel.
    pub const fn samples_per_frame(&self) -> u64 {
        self.channels as u64
    }
}

/// One member's `data` payload, decoded.
///
/// `frames` counts whole declared frames; `samples` counts the values in them
/// (one per channel). `byte_len` is the payload the decode accounted for,
/// which equals `frames * frame_bytes`: a partial trailing frame is refused,
/// never padded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedSound {
    format: SampleFormat,
    frames: u64,
    samples: Vec<i32>,
}

impl DecodedSound {
    /// The declaration the samples were decoded under.
    pub const fn format(&self) -> &SampleFormat {
        &self.format
    }

    /// How many whole declared frames the payload holds.
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    /// How many sample values one frame holds.
    pub const fn samples_per_frame(&self) -> u64 {
        self.format.samples_per_frame()
    }

    /// How many sample values the payload holds in total.
    pub fn sample_count(&self) -> u64 {
        self.samples.len() as u64
    }

    /// How many bytes of `data` the decode accounted for.
    pub fn byte_len(&self) -> u64 {
        self.frames * self.format.frame_bytes()
    }

    /// The decoded values, frame by frame and channel by channel, widened to
    /// `i32` and otherwise exactly as stored.
    pub fn samples(&self) -> &[i32] {
        &self.samples
    }

    /// The values of one frame, in channel order, or `None` when the frame is
    /// past the end.
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

    /// The byte offset inside the member the failure is anchored at.
    pub fn offset(&self) -> u64 {
        match self {
            Self::Format(_) => 0,
            Self::PartialFrame { bytes, .. } => *bytes,
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
            Self::Parse(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SampleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Format(error) => Some(error),
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

/// Decodes `member` under the plan its own WAVE header declares.
///
/// `format` is the member's own [`SampleFormat`] — built by
/// [`SampleFormat::from_header`] from the header task #344 read — and
/// `member` is that member's whole bytes. The decode reads the `data` payload
/// the plan points at and nothing else.
///
/// This is the stage's minimum acceptance case (spec F06 AC03): the result's
/// `byte_len` is the payload's length, its `sample_count` is
/// `frames * nChannels`, and the two satisfy
/// `byte_len == sample_count * bytes_per_sample` because the constructor
/// checked `nBlockAlign` against the channel count and the sample width.
///
/// # Errors
///
/// [`SampleError::PartialFrame`] when the `data` payload is not a whole
/// number of declared frames, and [`SampleError::Parse`] when the sample
/// buffer does not fit the parse's allocation budget.
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
    let remainder = byte_len % frame_bytes;
    if remainder != 0 {
        return Err(SampleError::PartialFrame {
            bytes: byte_len,
            frame_bytes,
            remainder,
        });
    }
    let frames = byte_len / frame_bytes;
    let sample_count = frames * format.samples_per_frame();
    let container = context.container().to_owned();
    let offset = format.data_span().offset;

    context
        .parse(
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
                let sample_bytes =
                    usize::try_from(format.layout().bytes_per_sample()).unwrap_or(usize::MAX);
                let mut samples: Vec<i32> = Vec::with_capacity(capacity);
                // A frame is `nBlockAlign` bytes: `nChannels` interleaved
                // samples, so a multi-channel frame yields `nChannels` values, not
                // one.
                for frame in
                    payload.chunks_exact(usize::try_from(frame_bytes).unwrap_or(usize::MAX))
                {
                    for sample in frame.chunks_exact(sample_bytes) {
                        match format.layout() {
                            PcmLayout::Unsigned8 => samples.push(i32::from(sample[0])),
                            PcmLayout::Signed16Le => {
                                samples.push(i32::from(i16::from_le_bytes([sample[0], sample[1]])))
                            }
                            PcmLayout::Signed32Le => samples.push(i32::from_le_bytes([
                                sample[0], sample[1], sample[2], sample[3],
                            ])),
                        }
                    }
                }
                debug_assert_eq!(samples.len(), capacity, "one value per sample per channel");
                debug_assert_eq!(samples.len() as u64, sample_count);
                Ok(DecodedSound {
                    format: *format,
                    frames,
                    samples,
                })
            },
        )
        .map_err(SampleError::from)
}
